use std::path::PathBuf;

/// FNV-1a 的初始偏移与质数。抽出成常量, 避免 6 处偏移/质数各写一遍而改一处漏一处。
pub(super) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub(super) const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 把一段字节混入 FNV-1a。
pub(super) fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 列间分隔符: 没有它, `[1,2] + [3]` 与 `[1] + [2,3]` 的列字节流会相同。
pub(super) const FNV_COL_SEP: u8 = 0xff;

/// **数据内容**的指纹 (FNV-1a over 六列的原始位模式)。
///
/// 存在的理由: 缓存文件名原本只混入 `window`(start_date/end_date 的**字符串**)、
/// 资金与执行口径, **不含数据本身**。可触发的错误场景: 同一 `start~end` 窗口下
/// 先跑一次(此时该目录只有 1 月数据), 再补进 2 月数据后重跑 —— `window` 字符串
/// 一字未变, 于是**全部组合命中旧缓存**, 报告里的 bars 数/指标仍基于只有 1 月的
/// 数据, 而使用者以为跑的是完整窗口。这类错误不会报错, 只会让研究结论悄悄过期。
///
/// 为什么用**内容哈希**而非文件 mtime / 目录递归:
/// - 目录 mtime 只反映"条目增删", 覆盖不到"同路径文件被原地改写"
/// - 递归遍历依赖文件系统语义, 换数据源/挂载方式就失效
/// - 列已全量载入内存, 直接哈希成本 ~20ms/1M bars, 无需再访问文件系统
pub(super) fn data_fingerprint(cols: &crate::data::KlineColumns) -> u64 {
    let mut h = FNV_OFFSET;
    // 列名混入: 将来增删列时指纹必然改变, 否则两套不同列结构可能撞指纹。
    for name in ["time", "open", "high", "low", "close", "volume"] {
        h = fnv_bytes(h, name.as_bytes());
        h = fnv_bytes(h, &[FNV_COL_SEP]);
    }
    h = fnv_bytes_i64(h, cols.time());
    for col in [
        cols.open(),
        cols.high(),
        cols.low(),
        cols.close(),
        cols.volume(),
    ] {
        h = fnv_bytes_f64(h, col);
    }
    h
}

pub(super) fn fnv_bytes_i64(mut h: u64, xs: &[i64]) -> u64 {
    for &x in xs {
        // `to_le_bytes` 而非 `to_ne_bytes`: 指纹必须**跨机器可复现**。
        // 用本机字节序会让同一份数据在 x86 与 ARM 上得到不同指纹 → 缓存
        // 在异构机器间无法复用; 且与相邻 `fnv_bytes_f64` 的编码风格也不一致。
        h = fnv_bytes(h, &x.to_le_bytes());
    }
    fnv_bytes(h, &[FNV_COL_SEP])
}

pub(super) fn fnv_bytes_f64(mut h: u64, xs: &[f64]) -> u64 {
    for &x in xs {
        h = fnv_bytes(h, &x.to_bits().to_le_bytes());
    }
    fnv_bytes(h, &[FNV_COL_SEP])
}

/// 本次运行要产出的**文件集**开关。
///
/// 进缓存指纹: 产出集不同 ⇒ 缓存身份不同 ⇒ 必须重算(见 [`params_fingerprint`]).
///
/// 为什么值得单独成类型: 这两个开关是"**产物集合**"的完整描述, 与"数据/口径"
/// 属于不同的关注点。合成一个值对象后, 指纹函数的参数个数不超 clippy 上限,
/// 且"漏传某一个开关"这类错误在类型上更难发生(要漏就得漏两个)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct OutputSet {
    /// 是否写逐笔交易与权益曲线(`--detail`)。
    pub(super) detail: bool,
    /// 是否写 fixed 腿逐 bar 曲线(`--pbo` 的唯一输入)。
    pub(super) fixed_curves: bool,
}

/// 参与缓存指纹的**代码版本**维度。
///
/// 编译期常量, 由 `build.rs` 对 `src/` 与两个 path 依赖的 `src/` 做内容哈希
/// 后注入(`cargo:rustc-env=VTA_SOURCE_HASH`)。
///
/// **为什么必须有它**: 指纹的其余维度都是"输入"(数据/资金/窗口/口径/参数),
/// 不含"**实现**"。于是改完策略代码再重跑会**全部命中旧缓存**, 报告里仍是
/// 旧代码的结果 —— 且**无任何提示**。这正是研究工具里最典型的"结论悄悄过期"
/// 失效模式。
///
/// 注入失败(找不到任何源文件)时 build.rs 会给出哨兵值 `unknown` 并告警;
/// 此时该维度**不随代码变化**, 退化为"没有代码版本维度"。
#[must_use]
pub fn source_hash() -> &'static str {
    env!("VTA_SOURCE_HASH")
}

/// 参数集合的稳定指纹 (FNV-1a)。写入组合文件名,避免改动参数后误命中旧缓存。
///
/// 混入的每一项都对应一类"改了但结果会变"的输入:
/// - `data_fp`: 行情内容本身(见 [`data_fingerprint`])
/// - `balance`: 资金口径
/// - `window`: start_date/end_date 字符串
/// - `exec`: delay / fee / slippage / fill 价(前视乐观与口径差异会整体改写结果)
/// - `outputs`: 产出文件集(见 [`OutputSet`])
/// - 实现版本: 代码内容哈希(见 `source_hash`)
///
/// **为什么实现版本也必须进指纹**: 改完策略/指标代码后重跑, 若指纹不变则
/// **全部命中旧缓存** —— 报告里是修复前的结果, 而使用者以为拿到了新结果。
/// 这是"结论悄悄过期"最隐蔽的一种: 连日志都不会提到缓存命中。
///
/// **为什么产出开关也必须进指纹**: 缓存命中会**直接早退**, 根本走不到
/// detail 分支 —— 而 detail 产物(逐笔交易 / 权益曲线 / fixed 曲线)只由该分支
/// 写出。若开关不进指纹, 则"先 `detail=false` 跑一次, 再 `detail=true` 跑第二次"
/// 会**全部命中缓存**, `detail/` 目录一个文件都没有, 而用户以为自己拿到了
/// detail 数据; 紧接着 `vta --pbo` 报"未找到 *_fixedcurve.parquet" —— 死循环。
/// 指纹的定义即"这次要产出什么", 产出集不同 ⇒ 身份不同 ⇒ 必然重算。
#[cfg(test)]
pub(super) fn params_fingerprint(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
) -> u64 {
    params_fingerprint_with_stop(
        strategy,
        n,
        balance,
        window,
        data_fp,
        exec,
        outputs,
        &crate::engine::StopConfig::default(),
        &crate::engine::SizingConfig::default(),
    )
}

/// 带止损的指纹。止损会改变成交序列与指标, 必须进入缓存身份; 否则同一策略
/// 换 stop 后会**命中旧缓存**, 报告里仍是旧止损的结果。
#[allow(clippy::too_many_arguments)]
pub(super) fn params_fingerprint_with_stop(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
    stop: &crate::engine::StopConfig,
    sizing: &crate::engine::SizingConfig,
) -> u64 {
    params_fingerprint_with_source_and_stop(
        strategy,
        n,
        balance,
        window,
        data_fp,
        exec,
        outputs,
        stop,
        sizing,
        source_hash(),
    )
}

/// [`params_fingerprint`] 的可测版本: 实现版本维度作为**参数**传入。
///
/// 抽出参数是为了让"指纹是否真的含实现版本"这件事**可测**:
/// 测试可以拿 `""`(空实现版本)算一遍, 断言与真实指纹**不同** ——
/// 否则"实现版本这一维被漏混进去"无法被任何测试发现。
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn params_fingerprint_with_source(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
    source: &str,
) -> u64 {
    params_fingerprint_with_source_and_stop(
        strategy,
        n,
        balance,
        window,
        data_fp,
        exec,
        outputs,
        &crate::engine::StopConfig::default(),
        &crate::engine::SizingConfig::default(),
        source,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn params_fingerprint_with_source_and_stop(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
    stop: &crate::engine::StopConfig,
    sizing: &crate::engine::SizingConfig,
    source: &str,
) -> u64 {
    let mut h = fnv_bytes(FNV_OFFSET, &data_fp.to_le_bytes());
    // 金额混入其**十进制表示**(精确): 若用 `f64` 位模式, 两个不同的 Decimal
    // 可能映射到同一个 f64 ⇒ 指纹碰撞 ⇒ 误命中旧缓存。
    h = fnv_bytes(h, balance.decimal().to_string().as_bytes());
    h = fnv_bytes(h, window.as_bytes());
    h = fnv_bytes(h, &(exec.delay() as u64).to_le_bytes());
    h = fnv_bytes(h, &exec.fee_rate.to_bits().to_le_bytes());
    h = fnv_bytes(h, &exec.slip_rate.to_bits().to_le_bytes());
    h = fnv_bytes(h, &[u8::from(exec.fill_close)]);
    // 产出集开关: detail 决定是否写逐笔交易/权益曲线, fixed_curves 决定是否写
    // fixed 曲线(`--pbo` 的唯一输入)。两者都改变落盘的文件集合, 故进指纹。
    h = fnv_bytes(h, &[u8::from(outputs.detail)]);
    h = fnv_bytes(h, &[u8::from(outputs.fixed_curves)]);
    // 实现版本: 代码一变, 指纹必变 ⇒ 旧缓存必然不命中。
    h = fnv_bytes(h, source.as_bytes());
    h = fnv_bytes(h, &[FNV_COL_SEP]);
    // 止损/仓位是运行口径的一部分: 同一策略同一参数换 stop/sizing 必须重算。
    let stop_params = stop.param_str();
    let size_params = sizing.param_str();
    h = fnv_bytes(h, stop_params.as_bytes());
    h = fnv_bytes(h, &[FNV_COL_SEP]);
    h = fnv_bytes(h, size_params.as_bytes());
    h = fnv_bytes(h, &[FNV_COL_SEP]);
    for i in 0..n {
        // 与报告行主键 `stop.apply_to(param_str, sizing)` 逐字节等价, 但避免
        // 每档多分配一个 `base;stop,size` 字符串(FNV 逐字节, 拼接哈希可分段喂入)。
        let base = strategy.param_str(i);
        h = fnv_bytes(h, base.as_bytes());
        if !stop_params.is_empty() {
            h = fnv_bytes(h, b";");
            h = fnv_bytes(h, stop_params.as_bytes());
        }
        if !size_params.is_empty() {
            h = fnv_bytes(h, if stop_params.is_empty() { b";" } else { b"," });
            h = fnv_bytes(h, size_params.as_bytes());
        }
        h = fnv_bytes(h, &[FNV_COL_SEP]);
    }
    h
}

pub(super) fn combo_parquet_path(
    temps_dir: &std::path::Path,
    symbol: &str,
    tf: &str,
    strategy: &str,
    fp: u64,
) -> PathBuf {
    temps_dir.join(format!("{symbol}_{tf}_{strategy}_{fp:016x}.parquet"))
}
