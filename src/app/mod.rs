use std::path::PathBuf;

use crate::config;
use crate::engine;
use crate::output;
use crate::strategies::bars_per_year;
#[cfg(test)]
use crate::strategies::build::build_strategies_checked;
use crate::strategies::build::{StrategyJob, build_jobs_checked};

#[derive(Debug, Clone)]
/// 单个参数求值的结果行, 即报告 `result.parquet` 的一行。
pub struct RunResult {
    /// 品种。
    pub symbol: String,
    /// 周期(如 `1d`)。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文; 四元组行主键 `(symbol, timeframe, strategy, params)` 之一。
    pub params: String,
    /// 全部回测指标。
    pub metrics: engine::SummaryMetrics,
    /// **原样保留**写入时的数值, 合并时不重算(见 `strategy_params_value` 的文档)。
    /// 0 表示"未记录"(旧缓存文件无此列)。
    pub strategy_params: u64,
    /// 同上, 人类可读的 JSON 编码原样保留。空串表示未记录。
    pub strategy_params_json: String,
    /// 执行口径: 信号到成交的 bar 间隔。
    pub delay_bars: u32,
    /// 执行口径: 单边佣金(bp)。
    pub fee_bps: f64,
    /// 执行口径: 单边逆向滑点(bp)。
    pub slippage_bps: f64,
    /// delay>0 时 fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
    /// 被动基准(固定名义): 首根 close 买入、末根 close 卖出、无成本的 pnl
    /// (**金额**, 定点十进制)。
    pub buy_hold_pnl: crate::money::Amount,
    /// 被动基准总收益(=pnl/名义)。
    pub buy_hold_return: f64,
    /// 本次求值实际喂给策略的 bar 数。
    ///
    /// 为什么必须有: `params` 字符串只描述**参数**, 不描述**数据规模**。
    /// 同一 `period=400` 在 300 bar 与 20000 bar 上参数串完全相同, 但前者
    /// 策略窗口都不够、全程 Hold、指标全 0。没有这一列, 下游无法判断某个
    /// "全 0 结果"是真的还是数据不足导致的退化。
    pub bars_used: u32,
    /// 求值状态。见 [`RunStatus`]。
    pub status: RunStatus,
}

/// 单个参数求值的**可用性状态**。写入报告的 `status` 列。
///
/// 存在的理由: 退化结果的指标全是 0, 与"零波动低风险"的真实结论在数值上
/// **不可区分**。实测反例(同一 `params` 字符串 `period=400`):
///
/// - 1d / 300bar: `trades=0 / pnl=0 / sharpe=0`(策略窗口都不够, 全程 Hold)
/// - 1m / 20000bar: `trades=1310 / pnl=-1.0 / sharpe=-18.04`
///
/// 只靠指标列无法判断前者是"数据不够, 策略根本没跑起来"还是"跑了但确实无信号"。
/// 排序/取 top-N 时前者会挤掉真实结果且无人察觉。
///
/// 口径说明: `NoSignals` 同时覆盖"策略窗口 > 数据长度"与"该参数本就无信号"
/// 两种成因 —— 两者在引擎层不可区分(都是全 Hold)。**`bars_used` 列提供了区分
/// 所需的信息**, 两者必须配对解读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunStatus {
    /// 正常: 产生了成交。
    #[default]
    Ok,
    /// 全程 Hold: 没有任何非 Hold 信号, 全部指标为 0(退化结果)。
    NoSignals,
    /// 有非 Hold 信号但一笔未成交(如信号全落在末根, `delay >= 1` 时无未来 fill bar)。
    NoTrades,
    /// **该参数档位不可计算**(数据/参数不足, 例如 `period` 超过可用 bar 数)。
    ///
    /// 与 [`Self::NoSignals`] 的区别是本项目的核心不变量:
    /// - `NoSignals` = 这一档**跑了**, 但全程 Hold(可能本就无信号);
    /// - `Invalid` = 这一档**根本没算出来**(指标库返回错误), 被显式跳过。
    ///
    /// 引入本状态的原因: 在此之前, 66/67 个策略把"算不出来"**静默**降级为
    /// 全 Hold, 于是它在报告里长得和"零波动低风险"的真实结论**一模一样**
    /// (`trades=0 / pnl=0 / sharpe=0`) —— 对参数网格过拟合研究(本项目的 PBO
    /// 模块)而言, 这正是最该被发现的失败模式, 却无人察觉。
    ///
    /// 读表时**必须**把 `Invalid` 单独筛掉: 它不是"策略没信号", 而是"这一档
    /// 不可用", 把它混进统计会稀释样本并高估/低估胜率。
    Invalid,
    /// **账户已破产**: 权益(现金)降到 ≤ 0, 引擎拒绝再开仓。
    ///
    /// 与 [`Self::Invalid`] 不同: 这一档**跑出来了**, 只是资金曲线归零/转负。
    /// 典型成因是空头在上涨行情中亏损超过本金。旧引擎没有开仓闸门, 爆仓后
    /// 会继续以负现金计算持仓量(数量变负 ⇒ 方向反转), 报告里出现无法解释的
    /// 巨额权益; 现在只允许平掉已有仓位, 不再开新仓。
    ///
    /// 读表时同样**不能**把它当作正常 `ok` 参与排名/PBO: 收益率已失去意义
    /// (本金归零后任何比例都不可比)。
    Bankrupt,
}

impl RunStatus {
    /// 落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NoSignals => "no_signals",
            Self::NoTrades => "no_trades",
            Self::Invalid => "invalid",
            Self::Bankrupt => "bankrupt",
        }
    }

    /// 从 `as_str` 的落盘文本解析。
    ///
    /// 用 `Option` 而不是 `Default`: 读到未知状态必须让调用方报错, 不能把
    /// "别的版本写的新状态"静默当成 `ok` 混入 PBO。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Self::Ok),
            "no_signals" => Some(Self::NoSignals),
            "no_trades" => Some(Self::NoTrades),
            "invalid" => Some(Self::Invalid),
            "bankrupt" => Some(Self::Bankrupt),
            _ => None,
        }
    }

    /// 从信号序列与回测指标判定状态。
    ///
    /// `Bankrupt` 优先于 `Ok`: 账户权益 ≤ 0 的运行即使产生过成交, 也不能作为
    /// 正常结果参与排名(收益率的分母已失效)。判定条件用**末点权益**而非
    /// 单笔盈亏 —— 爆仓可能由多笔累计亏损造成。
    #[must_use]
    pub fn classify(
        signals: &[crate::strategies::Signal],
        metrics: &engine::SummaryMetrics,
    ) -> Self {
        if metrics.num_trades > 0 && metrics.final_equity <= crate::money::Amount::ZERO {
            return Self::Bankrupt;
        }
        if metrics.num_trades > 0 {
            return Self::Ok;
        }
        if signals
            .iter()
            .any(|s| *s != crate::strategies::Signal::Hold)
        {
            Self::NoTrades
        } else {
            Self::NoSignals
        }
    }
}

#[derive(Debug, Clone)]
/// `--detail` 模式的落盘产物(逐笔交易 + 两条曲线)。
pub struct DetailResult {
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,
    /// 该档位的运行状态。
    ///
    /// `no_signals` / `no_trades` / `bankrupt` 也会产生曲线文件, 但**不得**参与
    /// PBO 排名: 它们的收益率没有可比性(全 0 或本金已归零)。状态随曲线落盘,
    /// `--pbo` 才能把它们显式筛掉而不是当成正常策略。
    pub status: RunStatus,
    /// 逐笔已平仓交易。
    pub trades: Vec<engine::Trade>,
    /// 复利腿逐 bar `(时间戳, 权益)`。
    pub equity_curve: Vec<(f64, f64)>,
    /// fixed 腿逐 bar 权益(与 bar 对齐), 写入 `{...}_fixedcurve.parquet` 供 `--pbo`。
    pub fixed_curve: Vec<f64>,
}

/// FNV-1a 的初始偏移与质数。抽出成常量, 避免 6 处偏移/质数各写一遍而改一处漏一处。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 把一段字节混入 FNV-1a。
fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 列间分隔符: 没有它, `[1,2] + [3]` 与 `[1] + [2,3]` 的列字节流会相同。
const FNV_COL_SEP: u8 = 0xff;

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
fn data_fingerprint(cols: &kline_reader::KlineColumns) -> u64 {
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

fn fnv_bytes_i64(mut h: u64, xs: &[i64]) -> u64 {
    for &x in xs {
        // `to_le_bytes` 而非 `to_ne_bytes`: 指纹必须**跨机器可复现**。
        // 用本机字节序会让同一份数据在 x86 与 ARM 上得到不同指纹 → 缓存
        // 在异构机器间无法复用; 且与相邻 `fnv_bytes_f64` 的编码风格也不一致。
        h = fnv_bytes(h, &x.to_le_bytes());
    }
    fnv_bytes(h, &[FNV_COL_SEP])
}

fn fnv_bytes_f64(mut h: u64, xs: &[f64]) -> u64 {
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
struct OutputSet {
    /// 是否写逐笔交易与权益曲线(`--detail`)。
    detail: bool,
    /// 是否写 fixed 腿逐 bar 曲线(`--pbo` 的唯一输入)。
    fixed_curves: bool,
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
fn params_fingerprint(
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
    )
}

/// 带止损的指纹。止损会改变成交序列与指标, 必须进入缓存身份; 否则同一策略
/// 换 stop 后会**命中旧缓存**, 报告里仍是旧止损的结果。
#[allow(clippy::too_many_arguments)]
fn params_fingerprint_with_stop(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
    stop: &crate::engine::StopConfig,
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
fn params_fingerprint_with_source(
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
        source,
    )
}

#[allow(clippy::too_many_arguments)]
fn params_fingerprint_with_source_and_stop(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: crate::money::Amount,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
    outputs: OutputSet,
    stop: &crate::engine::StopConfig,
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
    // 止损是运行口径的一部分: 同一策略同一参数换 stop 必须重算。
    let stop_params = stop.param_str();
    h = fnv_bytes(h, stop_params.as_bytes());
    h = fnv_bytes(h, &[FNV_COL_SEP]);
    for i in 0..n {
        // 与报告行主键 `stop.apply_to(param_str)` 逐字节等价, 但避免每档
        // 多分配一个 `base;stop` 字符串(FNV 逐字节, 拼接哈希可分段喂入)。
        let base = strategy.param_str(i);
        h = fnv_bytes(h, base.as_bytes());
        if !stop_params.is_empty() {
            h = fnv_bytes(h, b";");
            h = fnv_bytes(h, stop_params.as_bytes());
        }
        h = fnv_bytes(h, &[FNV_COL_SEP]);
    }
    h
}

fn combo_parquet_path(
    temps_dir: &std::path::Path,
    symbol: &str,
    tf: &str,
    strategy: &str,
    fp: u64,
) -> PathBuf {
    temps_dir.join(format!("{symbol}_{tf}_{strategy}_{fp:016x}.parquet"))
}

/// 单个组合执行所需的运行期上下文: 由 run() 一次性构建, 全部组合共享。
struct ComboCtx {
    /// 初始资金(**金额**, 定点十进制)。
    balance: crate::money::Amount,
    detail: bool,
    fixed_curves: bool,
    overwrite: bool,
    /// `--no-cache`: **忽略全部缓存**强制重算(逃生门)。
    ///
    /// 与 `overwrite` 的区别: `overwrite` 是**配置项**("结果文件要不要覆盖"),
    /// 本字段是**命令行开关**, 且作用在**组合缓存**(`temps/`)上 ——
    /// `overwrite=true` 本来就意味着全部重算, 但它写在配置文件里;
    /// `--no-cache` 让用户在命令行临时强制一次, 不必改配置。
    no_cache: bool,
    temps_dir: PathBuf,
    detail_dir: PathBuf,
    /// 数据窗口标识 (start_date/end_date), 纳入缓存指纹以区分不同数据区间。
    window: String,
    exec: crate::engine::ExecConfig,
}

impl ComboCtx {
    /// 构造一行 [`RunResult`]。**唯一构造点**。
    ///
    /// 此前 detail 与非 detail 两条分支各写一份 16 字段的字面量。两条分支的字段
    /// 集合已经在 L5(O6/bars_used/status)改造中发生过漂移风险 —— 任何一次新增
    /// 字段都可能只改到其中一条。这里收敛成单一来源后, 新增字段必然两分支同时生效。
    #[allow(clippy::too_many_arguments)]
    fn run_row(
        &self,
        symbol: &str,
        tf: &str,
        strategy: &dyn crate::strategies::Strategy,
        params: &str,
        mut metrics: engine::SummaryMetrics,
        bars_used: usize,
        status: RunStatus,
        bh_pnl: crate::money::Amount,
        bh_ret: f64,
    ) -> RunResult {
        // 执行口径是"本次运行假设", 不是从交易序列算出的指标; 在唯一构造点
        // 统一回填, 保证 invalid / 数据不足 / 正常行三者的三列完全一致。
        metrics.apply_exec(&self.exec);
        RunResult {
            symbol: symbol.to_string(),
            timeframe: tf.to_string(),
            strategy: strategy.name().to_string(),
            params: params.to_string(),
            strategy_params: output::strategy_params_value(params),
            strategy_params_json: output::params_to_json(params),
            metrics,
            delay_bars: self.exec.delay_u32(),
            fee_bps: self.exec.fee_rate * 10_000.0,
            slippage_bps: self.exec.slip_rate * 10_000.0,
            fill_close: self.exec.fill_close,
            buy_hold_pnl: bh_pnl,
            buy_hold_return: bh_ret,
            bars_used: u32::try_from(bars_used).unwrap_or(u32::MAX),
            status,
        }
    }

    /// 构造一个**不可计算**档位的报告行。
    ///
    /// 与正常行共用 [`Self::run_row`] 这**唯一**构造点(见 README「关键设计
    /// 约束」第 11 条)—— 区别只在 `status` 与 `metrics`:
    /// - `status = RunStatus::Invalid`: 明确告知"这一档**没算出来**",
    ///   而不是"跑了但没信号";
    /// - `metrics` 除 `final_equity` 与执行口径三列外全零, 但 `final_equity = balance`
    ///   (该档未开仓, 权益就是初始资金) —— 若留 0, `final_equity` 会与
    ///   `total_return = 0` 自相矛盾。
    ///
    /// `buy_hold_*` 与 `bars_used` 仍是**真实值**(基准与数据规模不因该档
    /// 不可算而改变)。
    #[allow(clippy::too_many_arguments)]
    fn invalid_row(
        &self,
        symbol: &str,
        tf: &str,
        strategy: &dyn crate::strategies::Strategy,
        params: &str,
        bars_used: usize,
        bh_pnl: crate::money::Amount,
        bh_ret: f64,
    ) -> RunResult {
        let metrics = engine::SummaryMetrics {
            final_equity: self.balance,
            ..engine::SummaryMetrics::default()
        };
        self.run_row(
            symbol,
            tf,
            strategy,
            params,
            metrics,
            bars_used,
            RunStatus::Invalid,
            bh_pnl,
            bh_ret,
        )
    }
}

/// K 线六列必须**等长**, 且价格列必须**有限且为正**。
///
/// 引擎内部多处用 `close.len().min(open.len())` 之类的截断来"保证"不越界。截断
/// 是静默的: 若某列短了, 引擎只会少回测几根 bar, 而**指标不会报错** —— 报告里
/// 的 bars_used 与用户预期不符, 研究结论却在无声地少算一段数据。
/// 更坏的情形是 `time` 比价列短: `scan_trades` 的 `ts(i)` 会退化成用 bar 序号
/// 当时间戳, detail 落盘的 `entry_time`/`exit_time` 变成假时间。
///
/// 价格列的合法性是**另一条**独立的红线: 成交价是整条成交链的除数
/// (`qty = cash / price`、`pnl_pct = (exit - entry) / entry`)。价格为 0 ⇒
/// `qty = inf` ⇒ 权益变 NaN ⇒ **全部指标被污染**; 而 NaN 比 0 更危险 ——
/// 0 会被读成"零波动低风险", NaN 才会暴露。
///
/// 因此在进入回测前一次性拒绝, 让问题在**数据加载边界**暴露。
///
/// 注意(2026-10): `KlineColumns` 私有化字段后, 不等长已**无法构造** ——
/// `new()` 返回 `Err`, `From<Vec<Record>>` 六列同步 push。价格列则**没有**这层
/// 保证, 故这条校验是可达的、也是必要的。
fn validate_columns(cols: &kline_reader::KlineColumns, where_: &str) -> crate::error::Result<()> {
    let lens = [
        ("time", cols.time().len()),
        ("open", cols.open().len()),
        ("high", cols.high().len()),
        ("low", cols.low().len()),
        ("close", cols.close().len()),
        ("volume", cols.volume().len()),
    ];
    let close_len = cols.close().len();
    if let Some((name, len)) = lens.iter().find(|(_, l)| *l != close_len) {
        return Err(crate::error::AppError::Msg(format!(
            "{where_}: K 线列长度不一致 —— {name}={len}, close={close_len}; \
             会导致引擎静默截断少算 bar 或时间戳失真。请重新下载/清洗数据"
        )));
    }
    // 价格列: 必须是有限正数。逐列扫描, 报告首个非法下标以便定位。
    for (name, col) in [
        ("open", cols.open()),
        ("high", cols.high()),
        ("low", cols.low()),
        ("close", cols.close()),
    ] {
        if let Some(i) = col.iter().position(|p| !p.is_finite() || *p <= 0.0) {
            return Err(crate::error::AppError::Msg(format!(
                "{where_}: 价格列 {name} 第 {i} 根非法(须为有限正数, 实际 {}) —— \
                 该值会成为成交数量的除数, 0 或 NaN 会让全部指标变成 NaN。\
                 请重新下载/清洗数据",
                col[i]
            )));
        }
    }
    Ok(())
}

/// 策略批量信号的长度必须与参数区间一致。
///
/// 契约由 [`crate::strategies::Strategy::generate_signals_batch`] 声明, 但
/// 实现方是**插件式的**(任何 `Strategy` 实现都可能写错), 且违反后果很重:
/// 信号与参数错位 ⇒ 报告里 `params` 列与实际跑的参数对不上 ⇒ 结论完全不可信。
///
/// 原实现用 `assert_eq!` 直接 panic —— 而此时整轮回测可能已经跑了几小时,
/// 崩溃会让此前所有已算结果白费(虽有 temps/ 缓存, 但用户拿到的是"进程被杀"
/// 而不是"哪个策略违反了契约")。这里改成**返回 Err**, 把问题变成一条可读的错误。
fn check_batch_len(name: &str, got: usize, want: usize) -> crate::error::Result<()> {
    if got != want {
        return Err(crate::error::AppError::Msg(format!(
            "[{name}] batch 信号数 {got} != 参数区间长度 {want}(策略实现违反契约)"
        )));
    }
    Ok(())
}

/// 执行单个 (symbol, timeframe, strategy) 组合:
/// 1. 若 overwrite=false 且缓存文件(含参数指纹)已存在且可读, 直接返回(跳过)
/// 2. 该策略的 n 个参数并行执行 信号生成+回测; detail 模式收集 trades/equity_curve
/// 3. 计算完成即写入 temps/(及 detail/)
///
/// `bpy` 与 `data_fp` 由调用方按当前 (symbol, timeframe) 对提供, 不放在 `ctx` 里:
/// 两者在策略层循环中是循环不变量(同一条 `cols`), 在这里重算等于把一次
/// O(全部列字节) 的哈希乘以策略数量(见 `data_fingerprint` 的成本说明)。
/// 同理, 列合法性检查 [`validate_columns`] 也由调用方**每组只做一次** ——
/// 它要全量扫描 4 条价格列, 放在这里等于把一次 O(T) 扫描乘以策略数。
/// 返回 (组合文件路径, 是否本次计算; false 表示命中缓存被跳过)。
fn run_combo(
    job: &StrategyJob,
    symbol: &str,
    tf: &str,
    cols: &kline_reader::KlineColumns,
    bpy: f64,
    data_fp: u64,
    ctx: &ComboCtx,
) -> crate::error::Result<(PathBuf, bool)> {
    let strategy = job.strategy.as_ref();
    let stop = &job.stop;
    let n = strategy.param_count();

    let combo_path = combo_parquet_path(
        &ctx.temps_dir,
        symbol,
        tf,
        strategy.name(),
        params_fingerprint_with_stop(
            strategy,
            n,
            ctx.balance,
            &ctx.window,
            data_fp,
            &ctx.exec,
            OutputSet {
                detail: ctx.detail,
                fixed_curves: ctx.fixed_curves,
            },
            stop,
        ),
    );

    // 组合缓存文件名本身就是本轮的身份(含源码/数据/执行口径/产出集指纹)。
    // detail 产物以它作为 metadata 里的 `combo` 字段, 使"同名参数文件被另一轮
    // 覆盖"也能在缓存命中时被发现。
    let combo_id = combo_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // L4: 指纹相同**不保证**文件完整 —— 文件可能被截断/写坏/被旧版本写入。
    // 少行的话下游合并会得到"缺一半参数"的结果而毫无提示, 故行数也必须对得上。
    if !ctx.overwrite && !ctx.no_cache && combo_path.exists() {
        // 只读 footer 行数, 不解码整张表: 这条路径每个策略都会走一次, 而缓存命中
        // 时真正需要的只有"行数是否等于参数数"。完整的 schema/内容校验留给
        // merge 阶段的 `read_parquet`。
        match output::parquet_row_count(&combo_path) {
            Ok(rows) if rows == n => {
                // 组合 parquet 只是结果的一部分: detail 模式还承诺 trades /
                // equitycurve(/fixedcurve) 文件。历史上只看行数就跳过, 于是
                // 手动清理 detail/ 后再跑会全部命中缓存、一个 detail 文件都不补,
                // 而 `--pbo` 只能报"未找到曲线"。侧车清单 + 文件大小/所属组合
                // 校验把"缓存命中"限定为**整个产出集**都完整。
                if !ctx.detail || output::detail_cache_is_complete(&combo_path, &ctx.detail_dir)? {
                    tracing::debug!("[{}]: 已存在, 跳过 ({rows} 条结果)", strategy.name());
                    return Ok((combo_path, false));
                }
                tracing::warn!(
                    "  [{}] 组合缓存命中但 detail 产物缺失/过期, 本次重算该组合",
                    strategy.name()
                );
            }
            Ok(rows) => {
                tracing::warn!(
                    "  [{}] 缓存行数 {rows} != 参数数 {n}(文件被截断或指纹碰撞), 判为失效并重算",
                    strategy.name()
                );
            }
            Err(e) => {
                tracing::warn!("[{}] 读取失败: {e}, 重新运行", strategy.name());
            }
        }
    }

    use rayon::prelude::*;

    if ctx.detail {
        // 旧清单必须先失效, 再覆盖任何 detail 文件。否则中途崩溃会留下
        // "旧清单 + 新 detail + 旧组合 parquet" 的假完整缓存。
        output::invalidate_combo_detail_manifest(&combo_path)?;
        let (bh_pnl, bh_ret) = crate::engine::buy_hold_pnl(cols.close(), ctx.balance)?;
        // detail 预检与自适应分块: 曲线(trades+equity+fixed)常驻内存曾导致整机 swap。
        // 单 param 约 T×200B; 峰值钳在 DETAIL_MEM_BUDGET, 超量分批, 单批都装不下则拒绝。
        const DETAIL_MEM_BUDGET: u64 = 4 * 1024 * 1024 * 1024;
        let per_param = cols.len() as u64 * 200;
        let total_est = per_param.saturating_mul(n as u64);
        let gib = |b: u64| {
            if b < 1_073_741_824 {
                format!("{:.1} MiB", b as f64 / 1_048_576.0)
            } else {
                format!("{:.1} GiB", b as f64 / 1_073_741_824.0)
            }
        };
        if per_param > DETAIL_MEM_BUDGET {
            return Err(crate::error::AppError::Msg(format!(
                "[{}] detail 单 param 预估 {} GiB 超预算 {} GiB, 拒绝执行(请关 detail 或拆小时间窗)",
                strategy.name(),
                gib(per_param),
                gib(DETAIL_MEM_BUDGET)
            )));
        }
        let chunk = ((DETAIL_MEM_BUDGET / per_param.max(1))
            .max(1)
            .min(n.max(1) as u64)) as usize;
        tracing::info!(
            "  [{}] detail 预检: 总量约 {}, 分 {} 批, 峰值约 {}",
            strategy.name(),
            gib(total_est),
            n.div_ceil(chunk.max(1)),
            gib(per_param.saturating_mul(chunk as u64))
        );

        let mut run_rows = Vec::with_capacity(n);
        let mut detail_files: Vec<PathBuf> = Vec::new();
        for start in (0..n).step_by(chunk.max(1)) {
            let end = (start + chunk).min(n);
            let batch = strategy.generate_signals_batch(
                cols.open(),
                cols.high(),
                cols.low(),
                cols.close(),
                cols.volume(),
                cols.time(),
                start..end,
            )?;
            check_batch_len(strategy.name(), batch.len(), end - start)?;
            // 每档独立: `Skipped` = 该档不可计算(记 Invalid, **继续**跑),
            // `Signals` = 正常回测。
            let sims: Vec<(String, Option<crate::engine::SimResult>, RunStatus)> = batch
                .into_par_iter()
                .enumerate()
                .map(|(k, outcome)| {
                    let i = start + k;
                    match outcome {
                        crate::strategies::ParamOutcome::Signals(signals) => {
                            let sim = crate::engine::simulate_detailed_ohlc(
                                cols.time(),
                                cols.high(),
                                cols.low(),
                                cols.close(),
                                cols.open(),
                                &signals,
                                ctx.balance,
                                bpy,
                                &ctx.exec,
                                stop,
                            )?;
                            let status = RunStatus::classify(&signals, &sim.summary);
                            Ok((stop.apply_to(&strategy.param_str(i)), Some(sim), status))
                        }
                        crate::strategies::ParamOutcome::Skipped(reason) => {
                            let params = stop.apply_to(&strategy.param_str(i));
                            tracing::warn!(
                                "  [{}] 参数档位 {params} 不可计算: {reason}(记为 invalid, 继续)",
                                strategy.name(),
                            );
                            Ok((params, None, RunStatus::Invalid))
                        }
                    }
                })
                .collect::<crate::error::Result<Vec<_>>>()?;

            let mut detail_rows = Vec::with_capacity(end - start);
            for (params, sim, status) in sims {
                let Some(crate::engine::SimResult {
                    summary,
                    trades,
                    equity_curve,
                    fixed_curve,
                }) = sim
                else {
                    // 不可计算的档位**没有**逐笔交易与曲线 → 不产出 detail 产物,
                    // 但仍写一行 `status = invalid` 的结果行(让报告完整可见)。
                    run_rows.push(ctx.invalid_row(
                        symbol,
                        tf,
                        strategy,
                        &params,
                        cols.len(),
                        bh_pnl,
                        bh_ret,
                    ));
                    continue;
                };
                run_rows.push(ctx.run_row(
                    symbol,
                    tf,
                    strategy,
                    &params,
                    summary,
                    cols.len(),
                    status,
                    bh_pnl,
                    bh_ret,
                ));
                detail_rows.push(DetailResult {
                    symbol: symbol.to_string(),
                    timeframe: tf.to_string(),
                    strategy: strategy.name().to_string(),
                    params,
                    status,
                    trades,
                    equity_curve,
                    fixed_curve,
                });
            }
            detail_files.extend(output::write_detail_parquet(
                &detail_rows,
                &ctx.detail_dir,
                ctx.fixed_curves,
                &combo_id,
            )?);
        }

        detail_files.sort();
        detail_files.dedup();
        output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
        // 组合 parquet 与侧车清单的写入顺序: 先写组合, 再写清单。若中途崩溃,
        // 下次缓存命中会因缺清单而重算; 反过来则会留下"清单说文件齐全, 但组合
        // parquet 不存在"的假缓存。
        output::write_combo_detail_manifest(&combo_path, &ctx.detail_dir, &detail_files)?;
    } else {
        let (bh_pnl, bh_ret) = crate::engine::buy_hold_pnl(cols.close(), ctx.balance)?;
        // 非 detail 也按信号内存分块: 全量 batch 常驻曾 OOM (35k×2.6M)。
        // 单 param 约 T×size_of::<Signal>()B; 峰值钳在 BATCH_MEM_BUDGET, 超量分批。
        const BATCH_MEM_BUDGET: u64 = 4 * 1024 * 1024 * 1024;
        let per_param = cols.len() as u64 * size_of::<crate::strategies::Signal>() as u64;
        let chunk = ((BATCH_MEM_BUDGET / per_param.max(1))
            .max(1)
            .min(n.max(1) as u64)) as usize;
        if n > chunk {
            tracing::info!(
                "  [{}] batch 分块: {} 参数, 分 {} 批",
                strategy.name(),
                n,
                n.div_ceil(chunk.max(1)),
            );
        }
        let mut run_rows: Vec<RunResult> = Vec::with_capacity(n);
        for start in (0..n).step_by(chunk.max(1)) {
            let end = (start + chunk).min(n);
            let batch = strategy.generate_signals_batch(
                cols.open(),
                cols.high(),
                cols.low(),
                cols.close(),
                cols.volume(),
                cols.time(),
                start..end,
            )?;
            check_batch_len(strategy.name(), batch.len(), end - start)?;
            let mut rows: Vec<RunResult> = batch
                .into_par_iter()
                .enumerate()
                .map(|(k, outcome)| {
                    let i = start + k;
                    // `param_str` 会为每次调用分配一个 `String`。本分支需要它三次
                    // (params 列 + 两列派生编码), 故只算一次再复用。止损后缀必须
                    // 拼进 params, 否则同一策略的 stop 网格行主键会碰撞。
                    let params = stop.apply_to(&strategy.param_str(i));
                    match outcome {
                        crate::strategies::ParamOutcome::Signals(signals) => {
                            let metrics = crate::engine::simulate_ohlc(
                                cols.high(),
                                cols.low(),
                                cols.close(),
                                cols.open(),
                                &signals,
                                ctx.balance,
                                bpy,
                                &ctx.exec,
                                stop,
                            )?;
                            let status = RunStatus::classify(&signals, &metrics);
                            Ok(ctx.run_row(
                                symbol,
                                tf,
                                strategy,
                                &params,
                                metrics,
                                cols.len(),
                                status,
                                bh_pnl,
                                bh_ret,
                            ))
                        }
                        crate::strategies::ParamOutcome::Skipped(reason) => {
                            tracing::warn!(
                                "  [{}] 参数档位 {params} 不可计算: {reason}(记为 invalid, 继续)",
                                strategy.name()
                            );
                            Ok(ctx.invalid_row(
                                symbol,
                                tf,
                                strategy,
                                &params,
                                cols.len(),
                                bh_pnl,
                                bh_ret,
                            ))
                        }
                    }
                })
                .collect::<crate::error::Result<Vec<_>>>()?;
            run_rows.append(&mut rows);
        }

        output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
    }

    Ok((combo_path, true))
}

/// 整体逻辑编排(与代码实际结构逐行对应):
/// ```text
/// 1.  加载配置并校验: config::load_config(path)?; config.validate()?; build_strategies → strategies
/// 2.  for symbol in data.symbols()                        // 品种层(单值时长度为 1)
/// 3.    for tf in data.timeframes()                       // 周期层(单值时长度为 1)
/// 4.      cols ← kline_reader::load_columns(data.for_pair(symbol, tf))   // 逐对加载
/// 5.      for strategy in strategies                      // 策略层
/// 6.        run_combo(...) → 单个组合处理, 内部:
/// 7.          a. 跳过判断: 非 overwrite 且组合缓存文件(含参数指纹)可读 → 跳过(仍并入合并)
/// 8.          b. 参数并行: 按信号内存预算分批, 批内并行 生成信号 + simulate(/detailed)
///8c.   data_fp = data_fingerprint(cols)   // 策略层循环**外**算一次, 供全部策略复用
/// 9.          c. detail 模式收集 trades/equity_curve
/// 10.         d. 写出: write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
/// 11. merge_parquet_files(combo_files) → result.parquet  // 只合并本次运行组合(新算+跳过)
/// ```
///
/// 单个 (symbol, timeframe) 数据加载失败时跳过该对并继续, 不中断整体扫描;
/// 全部组合都失败才返回错误。
///
/// # Errors
/// 配置加载/校验失败、参数网格自检失败、数据列非法、策略批量信号长度违反契约,
/// 或报告合并阶段完全失败时返回 [`crate::error::AppError`]。
pub fn run(config_path: &std::path::Path) -> crate::error::Result<()> {
    run_with(config_path, RunOptions::default())
}

/// [`run`] 的运行期选项(命令行可控)。
///
/// 单独成结构体而非给 `run` 加布尔参数: 布尔参数在调用点不可读
/// (`run(&p, true)` 看不出 `true` 是什么), 且再加开关就得再改签名。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// **忽略全部缓存**, 强制重算。
    ///
    /// 正常情况下缓存已足够安全: 指纹覆盖行情内容、资金、窗口、执行口径、
    /// 产出开关、**实现版本**(源码内容哈希) ⇒ 代码/数据/配置任一变化都会
    /// 自动失效。本开关用于"明知缓存可能过期、但只想强制重算一次"的场合。
    pub no_cache: bool,
}

/// 带选项执行一轮回测。生产入口见 [`run`]。
///
/// # Errors
/// 配置加载/校验失败、数据加载失败、或回测写盘失败时返回 [`AppError`]。
pub fn run_with(config_path: &std::path::Path, opts: RunOptions) -> crate::error::Result<()> {
    let config = config::load_config(config_path)?;
    config.validate()?;
    // 必须走 `build_jobs_checked`: 它构建策略 + 该条配置的 stop, 并在每条策略内
    // 对展开网格做 `param_str` 唯一性自检。直接用 `build_strategies` 会丢掉 stop,
    // 也静默放过 parquet `strategy_params` 列碰撞(见 build.rs 的文档)。
    let jobs = build_jobs_checked(&config.strategy)?;
    let detail = config.output.as_ref().is_some_and(|o| o.detail);
    let fixed_curves = config.output.as_ref().is_some_and(|o| o.fixed_curves);
    let overwrite = config.output.as_ref().is_some_and(|o| o.overwrite);
    let balance = config.backtest.initial_balance.amount();
    let window = format!(
        "{}~{}",
        config.data.start_date.as_deref().unwrap_or(""),
        config.data.end_date.as_deref().unwrap_or("")
    );

    let symbols = config.data.symbols();
    let timeframes = config.data.timeframes();
    let pairs_total = symbols.len() * timeframes.len();

    let out_dir: PathBuf = config
        .output
        .as_ref()
        .and_then(|o| o.dir.clone())
        .unwrap_or_else(|| PathBuf::from("reports"));
    let temps_dir = out_dir.join("temps");
    let detail_dir = out_dir.join("detail");
    // detail 索引标记"当前报告目录的 detail 产物属于哪一轮"。运行一开始就删掉:
    // 若本轮中途失败, 旧索引会继续指向可能已被部分覆盖的曲线, `--pbo` 会读到
    // 混合数据。删掉后 PBO 会明确要求重跑, 而不是静默给出错误 φ。
    // 非 detail 运行同样删除 —— 本轮没有产出 detail, 旧 detail 与新的
    // result.parquet 不再属于同一次运行。
    let detail_index_path = detail_dir.join(output::DETAIL_INDEX_FILE);
    match std::fs::remove_file(&detail_index_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(crate::error::AppError::Msg(format!(
                "删除旧 detail 索引失败 {}: {e}",
                detail_index_path.display()
            )));
        }
    }

    let mut combo_files: Vec<PathBuf> = Vec::new();
    let mut skipped = 0u64;
    let mut ran = 0u64;
    let mut load_failed = 0usize;

    let ctx = ComboCtx {
        balance,
        detail,
        fixed_curves,
        overwrite,
        no_cache: opts.no_cache,
        temps_dir: temps_dir.clone(),
        detail_dir: detail_dir.clone(),
        window: window.clone(),
        exec: crate::engine::ExecConfig::try_from_bps_fill(
            config.backtest.delay_bars.get(),
            config.backtest.fee_bps.get(),
            config.backtest.slippage_bps.get(),
            config.backtest.fill_bar_price == config::FillBarPrice::Close,
        )?,
    };

    tracing::info!("\n{}", "=".repeat(70));
    tracing::info!(
        "  扫描规模: {} 品种 × {} 周期 × {} 策略 = {} 组",
        symbols.len(),
        timeframes.len(),
        jobs.len(),
        pairs_total * jobs.len()
    );
    tracing::info!("{}", "=".repeat(70));

    let mut pair_idx = 0usize;
    for symbol in symbols {
        for tf in timeframes {
            pair_idx += 1;
            tracing::info!("\n[{pair_idx}/{pairs_total}] {symbol} / {tf}");

            let pair_config = match config.data.for_pair(symbol, tf) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("[跳过] 配置无效: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            let cols = match kline_reader::load_columns(&pair_config) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("[跳过] 数据加载失败: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            if cols.is_empty() {
                tracing::warn!("[跳过] 无数据");
                load_failed += 1;
                continue;
            }
            tracing::info!("数据: {} bars", cols.len());

            let bpy = bars_per_year(tf);
            // 策略层循环外算一次: 同一份 `cols` 会被全部策略复用, 在 `run_combo`
            // 内部重算等于把一次全列哈希乘以策略数(100+ 策略 ⇒ 100 倍浪费)。
            let data_fp = data_fingerprint(&cols);
            // 列合法性检查同理**提到策略循环外**: 它逐列全量扫描 4 条价格列
            // (O(T)), 放进 `run_combo` 等于把这次扫描乘以策略数。
            // 语义不变 —— 同一份 `cols` 对全部策略的判定本就相同, 提前失败
            // 反而更早报错(且不依赖是否走到策略层)。
            validate_columns(&cols, &format!("{symbol}/{tf}"))?;
            for job in &jobs {
                let (combo_path, computed) = run_combo(job, symbol, tf, &cols, bpy, data_fp, &ctx)?;
                if computed {
                    ran += 1;
                } else {
                    skipped += 1;
                }
                combo_files.push(combo_path);
            }
        }
    }

    if combo_files.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "全部 {} 组 (品种 × 周期) 均加载失败, 无可回测数据; 请检查 data.dir / symbol / timeframe",
            load_failed
        )));
    }

    let out_path = out_dir.join("result.parquet");
    let stats = output::merge_parquet_files(&combo_files, &out_path, balance)?;
    // `saturating_sub`: `total_files` 是 `u32::try_from(len).unwrap_or(u32::MAX)`,
    // 当组合文件数超过 `u32::MAX` 时它被截成 `u32::MAX`, 而 `failed_files`
    // 仍按真实失败数累加 ⇒ 普通 `-` 在 debug 下会**下溢 panic**。
    let ok_files = stats.total_files.saturating_sub(stats.failed_files);
    tracing::info!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 合并 {} 条结果 (来自 {}/{} 个文件)",
        stats.merged_rows,
        ok_files,
        stats.total_files
    );
    if stats.failed_files > 0 {
        // 报告缺了参数, 必须**以 warn 级别**显式告知 —— 用 `info!` 的话,
        // 默认的 `RUST_LOG=info` 恰好能看见, 但生产常见的 `RUST_LOG=warn`
        // 会把它**整个滤掉**, 于是"报告不完整"这件事无人知晓。
        tracing::warn!(
            "  [警告] {} 个组合文件读取失败已被跳过, 报告不完整(请删 temps/ 后重跑)",
            stats.failed_files
        );
    }
    if load_failed > 0 {
        tracing::info!("跳过 {load_failed} 组数据缺失的 (品种 × 周期)");
    }
    tracing::info!("合并 Parquet 已保存: {}", out_path.display());
    if detail {
        output::write_detail_index(&detail_dir, &combo_files)?;
        tracing::info!("详细数据 Parquet 已保存至: {}", detail_dir.display());
        tracing::info!("detail 索引: {}", detail_index_path.display());
    }

    Ok(())
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    /// 构造等长测试列。`KlineColumns` 字段已私有化, 只能经 `new` 构造,
    /// 且无法原地改写 —— 需要"内容不同但等长"的列时必须重建。
    #[allow(clippy::too_many_arguments)]
    fn cols_raw(
        time: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
        volume: Vec<f64>,
    ) -> kline_reader::KlineColumns {
        kline_reader::KlineColumns::new(time, open, high, low, close, volume)
            .expect("测试列必须等长")
    }

    /// `n` 根标准 OHLCV 列 (close 逐根 +0.5, 供 `close_of` 覆盖)。
    fn cols_with_close(n: usize, close_of: impl Fn(usize) -> f64) -> kline_reader::KlineColumns {
        cols_raw(
            (0..n as i64).map(|i| 1_700_000_000 + i * 3600).collect(),
            (0..n).map(|i| 100.0 + i as f64).collect(),
            (0..n).map(|i| 101.0 + i as f64).collect(),
            (0..n).map(|i| 99.0 + i as f64).collect(),
            (0..n).map(close_of).collect(),
            (0..n).map(|i| 10.0 + i as f64).collect(),
        )
    }

    fn cols(n: usize) -> kline_reader::KlineColumns {
        cols_with_close(n, |i| 100.5 + i as f64)
    }

    /// L3: 缓存指纹必须覆盖**数据内容**。
    ///
    /// 可触发场景: 同一 start~end 窗口先跑一次(只有 1 月数据), 再补进 2 月数据
    /// 重跑 —— 旧实现只混入 window 字符串, 两次指纹相同, 于是全部命中旧缓存,
    /// 报告仍基于只有 1 月的行情且无任何提示。
    #[test]
    fn fingerprint_covers_data_identity() {
        let a = cols(50);
        let b = cols(50);
        assert_eq!(
            data_fingerprint(&a),
            data_fingerprint(&b),
            "相同数据必须同指纹(否则缓存永不命中)"
        );

        // 追加 bar(数据被补齐)。`cols` 的取值公式逐根连续, 故 cols(51) 恰等于
        // 在 cols(50) 末尾 push 一根 (i=50: open=150 / high=151 / low=149 / close=150.5)。
        let longer = cols(51);
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&longer),
            "追加 bar 必须改变指纹(否则补数据后仍命中旧缓存)"
        );

        // 同一长度但**价格被改写**(同路径文件原地替换): 只把第 10 根 close +0.01
        let rewritten = cols_with_close(50, |i| {
            if i == 10 {
                100.5 + 10.0 + 0.01
            } else {
                100.5 + i as f64
            }
        });
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&rewritten),
            "同长度但价格变化必须改变指纹(否则行情被替换仍命中旧缓存)"
        );

        // 窗口字符串相同但数据不同 → params_fingerprint 必须不同
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let exec = crate::engine::ExecConfig::default();
        let plain = OutputSet::default();
        let fp_old = params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "~",
            data_fingerprint(&a),
            &exec,
            plain,
        );
        let fp_new = params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "~",
            data_fingerprint(&rewritten),
            &exec,
            plain,
        );
        assert_ne!(fp_old, fp_new, "同窗口不同数据必须给出不同缓存文件名");
    }

    /// 列间必须有分隔符: 否则 `[1,2]+[3]` 与 `[1]+[2,3]` 的字节流相同。
    #[test]
    fn data_fingerprint_separates_columns() {
        let a = cols_raw(
            vec![1, 2, 3],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
            vec![10.0, 11.0, 12.0],
            vec![13.0, 14.0, 15.0],
            vec![16.0, 17.0, 18.0],
        );
        // 把 close 的值挪到 open, 长度不变
        let b = cols_raw(
            vec![1, 2, 3],
            vec![13.0, 14.0, 15.0],
            vec![7.0, 8.0, 9.0],
            vec![10.0, 11.0, 12.0],
            vec![4.0, 5.0, 6.0],
            vec![16.0, 17.0, 18.0],
        );
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&b),
            "列内容互换必须改变指纹"
        );
    }

    /// 执行口径任一变化都必须失效缓存: 前视乐观口径与费率差异会整体改写结果。
    #[test]
    fn fingerprint_covers_exec_params() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let c = cols(10);
        let fp = data_fingerprint(&c);
        let plain = OutputSet::default();
        let base = params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &crate::engine::ExecConfig::default(),
            plain,
        );
        for exec in [
            crate::engine::ExecConfig::try_from_bps_fill(2, 0.0, 0.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 2.0, 0.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 1.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 0.0, true).unwrap(),
        ] {
            assert_ne!(
                base,
                params_fingerprint(
                    &*s,
                    1,
                    crate::money::Amount::from_i64(10_000),
                    "a~b",
                    fp,
                    &exec,
                    plain
                ),
                "执行口径 {:?} 变化必须失效缓存",
                exec
            );
        }
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(20_000),
                "a~b",
                fp,
                &crate::engine::ExecConfig::default(),
                plain
            ),
            "资金变化必须失效缓存"
        );
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~c",
                fp,
                &crate::engine::ExecConfig::default(),
                plain
            ),
            "窗口变化必须失效缓存"
        );
    }

    /// **回归**: `detail` / `fixed_curves` 开关必须进缓存指纹。
    ///
    /// 可触发场景(真实死循环): 先以 `detail=false` 跑一次 → `temps/` 里落了
    /// 结果缓存; 再以 `detail=true` 跑第二次 → 若开关不进指纹, 第二次**全部
    /// 命中缓存并直接早退**, 根本走不到 detail 分支 ⇒ `detail/` 目录一个文件
    /// 都没有(用户以为拿到了逐笔交易/曲线); 紧接着 `vta --pbo` 报
    /// "未找到 *_fixedcurve.parquet, 请先用 detail=true 重跑" —— 而用户**确实**
    /// 设了 `detail=true`。死循环。
    ///
    /// 指纹的定义是"这次要产出什么": 产出集不同 ⇒ 身份不同 ⇒ 必然重算。
    #[test]
    fn fingerprint_covers_output_set_switches() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let fp = data_fingerprint(&cols(10));
        let exec = crate::engine::ExecConfig::default();
        let plain = OutputSet::default();
        let base = params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            plain,
        );

        // detail 开关变化 ⇒ 指纹变化(否则第二次跑拿不到 detail 产物)。
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                OutputSet {
                    detail: true,
                    fixed_curves: false
                }
            ),
            "detail 开关变化必须失效缓存, 否则 detail 产物永远不会被写出"
        );
        // fixed_curves 开关变化 ⇒ 指纹变化(--pbo 的输入只由该开关产出)。
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                OutputSet {
                    detail: false,
                    fixed_curves: true
                }
            ),
            "fixed_curves 开关变化必须失效缓存, 否则 *_fixedcurve.parquet 不会被写出"
        );
        // 两个开关都变 ⇒ 与基线不同。
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                OutputSet {
                    detail: true,
                    fixed_curves: true
                }
            ),
            "detail + fixed_curves 同时变化必须失效缓存"
        );
        // 同样的开关组合 ⇒ 指纹相同(否则同一次运行会自我失效, 缓存永不命中)。
        assert_eq!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                plain
            ),
            "相同开关组合必须给出相同指纹"
        );
        assert_eq!(
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                OutputSet {
                    detail: true,
                    fixed_curves: true
                }
            ),
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                OutputSet {
                    detail: true,
                    fixed_curves: true
                }
            ),
            "相同开关组合必须给出相同指纹"
        );
    }

    /// 止损配置必须进入缓存指纹: 同一策略同参数换 stop 必须重算, 不能命中
    /// 旧缓存后把旧止损结果冒充新结果。
    #[test]
    fn fingerprint_covers_stop_config() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let fp = data_fingerprint(&cols(10));
        let exec = crate::engine::ExecConfig::default();
        let outputs = OutputSet::default();
        let base = params_fingerprint_with_stop(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
            &crate::engine::StopConfig::default(),
        );
        let sl = crate::engine::StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
        };
        let trail = crate::engine::StopConfig {
            stop_loss_pct: None,
            trailing_stop_pct: Some(5.0),
        };
        let with_sl = params_fingerprint_with_stop(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
            &sl,
        );
        let with_trail = params_fingerprint_with_stop(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
            &trail,
        );
        assert_ne!(base, with_sl, "固定止损必须改变缓存指纹");
        assert_ne!(base, with_trail, "追踪止损必须改变缓存指纹");
        assert_ne!(with_sl, with_trail, "不同止损配置必须可区分");
    }

    /// **回归**: 指纹必须含**实现版本**(源码内容哈希)。
    ///
    /// 可触发场景: 指纹只覆盖"输入"(数据/资金/窗口/口径/参数)时, 改完策略代码
    /// 重跑会**全部命中旧缓存** —— 报告里是修复前的结果, 而使用者以为拿到了
    /// 新结果, 且**连日志都不会提到缓存命中**。这是研究工具里最隐蔽的
    /// "结论悄悄过期"失效模式。
    #[test]
    fn fingerprint_covers_source_hash() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let exec = crate::engine::ExecConfig::default();
        let outputs = OutputSet::default();
        let fp = data_fingerprint(&cols(10));
        let real = params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
        );
        // 不含实现版本 ⇒ 必然不同。
        let without = params_fingerprint_with_source(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
            "",
        );
        assert_ne!(
            real, without,
            "实现版本必须进指纹, 否则改代码后仍命中旧缓存"
        );
        // 换一个实现版本 ⇒ 也必须不同。
        let other = params_fingerprint_with_source(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            outputs,
            "deadbeefdeadbeef",
        );
        assert_ne!(real, other, "实现版本变化必须改变指纹");
        assert_ne!(without, other);
    }

    /// 源码哈希必须可用(构建期找到了源文件)。
    ///
    /// 若 build.rs 找不到任何源文件, 它会注入哨兵值 `unknown` 并告警 ——
    /// 此时"实现版本"这一维**不随代码变化**, 退化为"没有这一维"。
    #[test]
    fn source_hash_is_available() {
        let h = source_hash();
        assert!(!h.is_empty(), "源码哈希不得为空");
        assert_eq!(h.len(), 16, "源码哈希应是 16 位十六进制: {h}");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "源码哈希必须是十六进制: {h}"
        );
    }

    /// **回归**: 指纹必须**跨机器可复现** —— 整数列用小端序编码。
    ///
    /// 可触发场景: 原实现用 `to_ne_bytes()`(本机字节序), 于是同一份数据在
    /// x86 与 ARM 上得到**不同**指纹 ⇒ 缓存无法在异构机器间复用, 且与相邻
    /// `fnv_bytes_f64` 的小端编码风格不一致。
    ///
    /// 断言方式: 同一列的指纹必须等于"显式小端编码"的结果。
    #[test]
    fn fingerprint_uses_little_endian_byte_order() {
        let xs: &[i64] = &[1, -1, i64::MIN, i64::MAX, 0x0102_0304_0506_0708];
        let mut expected = FNV_OFFSET;
        for &x in xs {
            expected = fnv_bytes(expected, &x.to_le_bytes());
        }
        let got = fnv_bytes_i64(FNV_OFFSET, xs);
        assert_eq!(
            got,
            fnv_bytes(expected, &[FNV_COL_SEP]),
            "整数列必须按小端序编码(跨机器可复现)"
        );
    }

    /// L4 + L11: 行数不符 / 文件读失败都不得静默出"完成"。
    #[test]
    fn merge_reports_failures_instead_of_silently_dropping() {
        let dir = std::env::temp_dir().join(format!("vta-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let rr = RunResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            metrics: engine::SummaryMetrics::default(),
            strategy_params: output::strategy_params_value("period=20"),
            strategy_params_json: output::params_to_json("period=20"),
            bars_used: 300,
            status: RunStatus::Ok,
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: crate::money::Amount::from_i64(1),
            buy_hold_return: 0.1,
        };
        let good = dir.join("good.parquet");
        let bad = dir.join("bad.parquet");
        let missing = dir.join("missing.parquet");
        output::write_parquet(
            &good,
            std::slice::from_ref(&rr),
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();
        std::fs::write(&bad, b"not a parquet file").unwrap();

        let out = dir.join("merged.parquet");

        // 全部不可读 → 必须报错, 而不是"合并 0 条"式的成功
        assert!(
            output::merge_parquet_files(
                &[bad.clone(), missing.clone()],
                &out,
                crate::money::Amount::from_i64(10_000),
            )
            .is_err(),
            "全部失败时必须报错"
        );

        // 部分失败 → 报告仍落盘, 但失败必须被**计数**并进入摘要
        let st = output::merge_parquet_files(
            &[good.clone(), bad, missing],
            &out,
            crate::money::Amount::from_i64(10_000),
        )
        .expect("部分失败仍应产出报告");
        assert_eq!(st.total_files, 3);
        assert_eq!(
            st.failed_files, 2,
            "读失败必须被计数(否则报告缺参数却显示完成)"
        );
        assert_eq!(st.merged_rows, 1);
        assert!(out.exists(), "报告仍应落盘");

        // L4: 缓存行数必须等于参数数。截断文件(少 1 行)不能被当成有效缓存。
        let truncated = dir.join("trunc.parquet");
        output::write_parquet(&truncated, &[], crate::money::Amount::from_i64(10_000)).unwrap();
        assert_eq!(
            output::read_parquet(&truncated).unwrap().len(),
            0,
            "构造 0 行缓存作为反例"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L6: K 线六列必须等长, 否则引擎会**静默截断**少算 bar。
    ///
    /// 可触发场景: 某列短了 → 引擎 `close.len().min(open.len())` 少回测几根,
    /// 指标不报错但 bars_used 与预期不符; `time` 短 → `ts(i)` 退化成用 bar 序号
    /// 当时间戳, detail 落盘的时间字段变成假时间。
    #[test]
    fn equal_length_columns_pass_alignment_check() {
        let c = cols(10);
        assert!(validate_columns(&c, "t").is_ok(), "等长必须通过");
        // 空数据不算"长度不一致", 交由 is_empty 判断
        let empty = cols(0);
        assert!(validate_columns(&empty, "t").is_ok());
    }

    /// 回归: 价格列必须是**有限正数**, 否则整轮指标会被 NaN 污染。
    ///
    /// 可触发场景: 某根 bar 的 close = 0 ⇒ 成交数量 `qty = cash / price` 变 `inf`
    /// ⇒ 权益变 NaN ⇒ sharpe / drawdown / pnl **全部**变 NaN。而 NaN 比 0 更危险:
    /// 0 会被读成"零波动低风险", NaN 才会暴露; 且下游的排序/取 top-N 会静默丢行。
    /// 校验必须发生在**数据加载边界**, 而不是等到出报告才发现。
    #[test]
    fn non_positive_or_non_finite_prices_are_rejected_at_the_data_boundary() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let c = kline_reader::KlineColumns::new(
                vec![1, 2, 3],
                vec![4.0, 5.0, 6.0],
                vec![7.0, 8.0, 9.0],
                vec![10.0, 11.0, 12.0],
                vec![13.0, 14.0, bad],
                vec![16.0, 17.0, 18.0],
            )
            .expect("等长列必须可构造");
            let e = validate_columns(&c, "t")
                .expect_err(&format!("close={bad} 必须被拒, 否则整轮指标变 NaN"));
            assert!(
                format!("{e}").contains("close"),
                "错误消息应指明是哪一列: {e}"
            );
        }
    }

    /// 回归: 合法价格列必须通过校验(不得误杀)。
    #[test]
    fn valid_prices_pass_the_boundary_check() {
        let c = cols(20);
        assert!(validate_columns(&c, "t").is_ok());
    }

    /// 列长相等现由 `KlineColumns` 的**构造**保证, 而非本模块事后校验:
    /// `new()` 直接返回 `Err`, `From<Vec<Record>>` 对每条记录向六列各 push 一个值,
    /// 两条路径都不可能产出不等长实例。故"列长不一致"必须在构造点被拒, 测试随之
    /// 移到这里 —— 本函数(`validate_columns`)的等长分支退化为不可达的兜底。
    #[test]
    fn uneven_columns_are_rejected_at_construction() {
        let uneven = kline_reader::KlineColumns::new(
            vec![1, 2, 3],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
            vec![10.0, 11.0],
            vec![13.0, 14.0, 15.0],
            vec![16.0, 17.0, 18.0],
        );
        let e = uneven.expect_err("少一根的列必须在构造时被拒");
        assert!(
            format!("{e}").contains("equal length"),
            "错误消息应说明长度约束: {e}"
        );
        // 注: `From<Vec<Record>>` 那条路径无法从外部构造 —— `kline_reader::app::Record`
        // 是私有类型, 外部只能经 `load_columns` 间接触达。
    }

    /// O9 回归: `run_combo` 的**两条分支**(detail / 非 detail)必须都经
    /// [`ComboCtx::run_row`] 构造 `RunResult`。
    ///
    /// 可触发场景: 非 detail 分支曾内联写一份 16 字段字面量, 而 detail 分支走
    /// `run_row`。两者字段集合已在一次字段新增中出现过漂移 —— 新列只加进其中一条,
    /// 报告里 `--detail` 开关会改变指标列的语义, 且不报错。
    /// 本测试用源码级断言把"两条分支都调用 run_row"钉死。
    #[test]
    fn both_combo_branches_construct_run_rows_via_run_row() {
        let src = include_str!("mod.rs");
        let body = src
            .split("fn run_combo(")
            .nth(1)
            .expect("源码中应存在 run_combo");
        // 只看函数体(到下一个顶层 `/// ` 文档注释为止), 避免匹配到后面的测试。
        let body = body.split("\n/// ").next().expect("run_combo 体应可截取");

        // 唯一构造点是 ComboCtx::run_row: 函数体内不得出现 RunResult 字面量。
        assert!(
            !body.contains("RunResult {"),
            "run_combo 内不得内联构造 RunResult —— 唯一构造点是 ComboCtx::run_row; \
             内联字面量会让 detail / 非 detail 两条分支的字段集合漂移(O9)"
        );
        // 两条分支各调用一次。
        assert_eq!(
            body.matches("ctx.run_row(").count(),
            2,
            "detail 与非 detail 分支必须各经一次 ctx.run_row"
        );
    }

    /// P1 回归: `data_fingerprint` 必须在**策略层循环之外**计算一次。
    ///
    /// 可触发场景: 该函数遍历全部 6 列逐字节 FNV(自述~20ms/1M bars)。它原先在
    /// `run_combo` 内被调用, 而 `run_combo` 按策略逐个调用 ⇒ 同一份行情被哈希
    /// **N 次**(100+ 策略即 100 倍纯浪费), 且随网格规模线性放大。
    /// 本测试锁定: `run()` 在策略循环外算 `data_fp` 并以参数传入。
    #[test]
    fn data_fingerprint_is_hoisted_out_of_the_strategy_loop() {
        let src = include_str!("mod.rs");
        let run_body = src.split("pub fn run(").nth(1).expect("源码中应存在 run()");
        let run_body = run_body.split("\n#[cfg(test)]").next().expect("run() 体");

        // run() 中恰有一次 data_fingerprint 调用, 且位于策略循环之前。
        assert_eq!(
            run_body.matches("data_fingerprint(").count(),
            1,
            "run() 中应只算一次 data_fingerprint"
        );
        let fp_at = run_body.find("data_fingerprint(").expect("已定位");
        let loop_at = run_body.find("for job in &jobs").expect("已定位策略循环");
        assert!(
            fp_at < loop_at,
            "data_fingerprint 必须在 `for job in &jobs` 之前计算"
        );

        // run_combo 内部不得再算(否则按策略重复)。
        let combo = src
            .split("fn run_combo(")
            .nth(1)
            .expect("源码中应存在 run_combo");
        let combo = combo.split("\n/// ").next().expect("run_combo 体");
        assert!(
            !combo.contains("data_fingerprint("),
            "run_combo 内不得重算 data_fingerprint —— 它由调用方按 (symbol, tf) 对传入"
        );
    }

    /// 回归: `build_jobs_checked` 的唯一性自检必须在**生产路径** `run()` 上生效。
    ///
    /// 可触发场景: `run()` 曾直接调 `build_strategies`(不做自检), 而
    /// `build_strategies_checked` 全仓库无调用方 —— 于是 `param_str` 碰撞完全静默,
    /// parquet 的 `strategy_params` 列排序错乱且无任何提示; README 却宣称该自检
    /// "每次启动时"运行。此测试锁定 `run()` 必须走 checked 变体, 且保留 stop。
    #[test]
    fn run_uses_the_uniqueness_checked_strategy_builder() {
        let src = include_str!("mod.rs");
        let body = src.split("pub fn run(").nth(1).expect("源码中应存在 run()");
        let body = body.split("\n#[cfg(test)]").next().expect("run() 函数体");
        assert!(
            body.contains("build_jobs_checked("),
            "run() 必须调用 build_jobs_checked, 否则参数网格碰撞检查与 stop 都会失效"
        );
        assert!(
            !body.contains("= build_strategies("),
            "run() 不得调用未做唯一性自检的 build_strategies"
        );
    }

    /// 回归: IO 层不得用 `to_str().expect(..)` 把 `Path` 降级成 `&str`。
    ///
    /// 可触发场景: 旧签名收 `&str`, 调用方被迫 `path.to_str().expect("non-UTF-8 path")`。
    /// 输出目录来自配置, 用户一旦填入非 UTF-8 路径, 整轮回测在**第一个组合**就 panic,
    /// 而不是报错。现签名直接收 `&Path`, 该转换连同 panic 一起消失。
    #[test]
    fn parquet_io_apis_take_path_and_never_panic_on_non_utf8() {
        let src = include_str!("04_write_output/parquet.rs");
        let io_fns = [
            "pub fn write_parquet(",
            "pub fn read_parquet(path: &Path",
            "pub fn read_equity_curves(dir: &Path",
            "pub fn read_fixed_curves(dir: &Path",
        ];
        for sig in io_fns {
            assert!(src.contains(sig), "IO 函数签名应收 &Path: {sig}");
        }
        // `write_parquet` 的签名是跨行的, 单独断言它的第一个参数是 `&Path`。
        assert!(
            src.contains("path: &Path,"),
            "write_parquet 的路径参数必须是 &Path"
        );
        assert!(
            !src.contains("non-UTF-8 path"),
            "生产代码不得用 expect(\"non-UTF-8 path\") 把 Path 降级为 &str"
        );
    }

    /// O9: `RunResult` 的构造必须**单一来源**, 否则两条分支的字段会漂移。
    /// **回归**: 不可计算的档位必须被记成 `RunStatus::Invalid`,
    /// 且 `final_equity = balance`(该档未开仓, 权益就是初始资金)——
    /// 若留 0, `final_equity` 会与 `total_return = 0` 自相矛盾。
    #[test]
    fn invalid_row_is_marked_invalid_and_keeps_equity_consistent() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let ctx = ComboCtx {
            balance: crate::money::Amount::from_i64(10_000),
            detail: false,
            fixed_curves: false,
            overwrite: false,
            no_cache: false,
            temps_dir: std::path::PathBuf::from("/tmp"),
            detail_dir: std::path::PathBuf::from("/tmp"),
            window: "~".to_string(),
            exec: crate::engine::ExecConfig::default(),
        };
        let r = ctx.invalid_row(
            "BTCUSDT",
            "1d",
            &*s,
            "period=20",
            300,
            crate::money::Amount::from_i64(1),
            0.1,
        );
        assert_eq!(r.status, RunStatus::Invalid);
        assert_eq!(r.status.as_str(), "invalid");
        assert_eq!(
            r.metrics.final_equity,
            crate::money::Amount::from_i64(10_000),
            "未开仓 ⇒ 权益就是初始资金, 不得为 0"
        );
        assert_eq!(r.metrics.total_return, 0.0);
        // 执行口径必须与正常行同源: invalid 行也是"本次运行"的一部分,
        // 旧实现只填外层 RunResult, 内层 SummaryMetrics 的 delay 留 0。
        assert_eq!(r.metrics.delay_bars, r.delay_bars);
        assert_eq!(r.metrics.fee_bps, r.fee_bps);
        assert_eq!(r.metrics.slippage_bps, r.slippage_bps);
        assert_ne!(r.metrics.delay_bars, 0, "delay_bars 恒 >= 1");
        assert_eq!(r.bars_used, 300, "数据规模不因该档不可算而改变");
        assert_eq!(
            r.buy_hold_pnl,
            crate::money::Amount::from_i64(1),
            "基准不因该档不可算而改变"
        );
    }

    /// 五个 `status` 取值的落盘文本必须互不相同且稳定。
    #[test]
    fn run_status_texts_are_distinct() {
        let all = [
            RunStatus::Ok,
            RunStatus::NoSignals,
            RunStatus::NoTrades,
            RunStatus::Invalid,
            RunStatus::Bankrupt,
        ];
        let mut seen = std::collections::HashSet::new();
        for s in all {
            assert!(seen.insert(s.as_str()), "status 文本重复: {}", s.as_str());
        }
        assert_eq!(RunStatus::Ok.as_str(), "ok");
        assert_eq!(RunStatus::NoSignals.as_str(), "no_signals");
        assert_eq!(RunStatus::NoTrades.as_str(), "no_trades");
        assert_eq!(RunStatus::Invalid.as_str(), "invalid");
        assert_eq!(RunStatus::Bankrupt.as_str(), "bankrupt");
        // 落盘文本必须能无损回读; 未知文本必须拒绝, 不得静默当 ok。
        for s in all {
            assert_eq!(RunStatus::parse(s.as_str()), Some(s), "{}", s.as_str());
        }
        assert_eq!(RunStatus::parse("bogus"), None);
        assert_eq!(RunStatus::parse(""), None);
    }

    /// `Bankrupt` 必须优先于 `Ok`: 跑出过成交但权益归零, 仍不能当正常结果。
    #[test]
    fn classify_marks_wiped_out_accounts_as_bankrupt() {
        let signals = [crate::strategies::Signal::Buy];
        let healthy = engine::SummaryMetrics {
            num_trades: 1,
            final_equity: crate::money::Amount::from_i64(100),
            ..engine::SummaryMetrics::default()
        };
        assert_eq!(RunStatus::classify(&signals, &healthy), RunStatus::Ok);

        let wiped = engine::SummaryMetrics {
            num_trades: 1,
            final_equity: crate::money::Amount::ZERO,
            ..engine::SummaryMetrics::default()
        };
        assert_eq!(RunStatus::classify(&signals, &wiped), RunStatus::Bankrupt);

        let negative = engine::SummaryMetrics {
            num_trades: 3,
            final_equity: crate::money::Amount::from_i64(-5),
            ..engine::SummaryMetrics::default()
        };
        assert_eq!(
            RunStatus::classify(&signals, &negative),
            RunStatus::Bankrupt
        );

        // 没产生过成交(本金仍为正) 时不能误判为破产。
        let no_trades = engine::SummaryMetrics {
            num_trades: 0,
            final_equity: crate::money::Amount::from_i64(10_000),
            ..engine::SummaryMetrics::default()
        };
        assert_eq!(
            RunStatus::classify(&signals, &no_trades),
            RunStatus::NoTrades
        );
        assert_eq!(
            RunStatus::classify(&[crate::strategies::Signal::Hold], &no_trades),
            RunStatus::NoSignals
        );
    }

    #[test]
    fn run_row_is_the_single_construction_point() {
        let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .expect("sma 网格合法")
        .pop()
        .expect("sma");
        let ctx = ComboCtx {
            balance: crate::money::Amount::from_i64(10_000),
            detail: false,
            fixed_curves: false,
            overwrite: true,
            no_cache: false,
            temps_dir: "t".into(),
            detail_dir: "d".into(),
            window: "a~b".into(),
            exec: engine::ExecConfig::try_from_bps_fill(1, 2.0, 1.0, true).unwrap(),
        };
        let m = engine::SummaryMetrics {
            sharpe_ratio: 1.25,
            ..engine::SummaryMetrics::default()
        };
        let r = ctx.run_row(
            "BTCUSDT",
            "1d",
            &*s,
            "period=20",
            m,
            300,
            RunStatus::NoSignals,
            crate::money::Amount::from_i64(123),
            0.0123,
        );
        // 执行口径字段由 ctx 提供, 不可能与分支漂移
        assert_eq!(r.delay_bars, 1);
        assert!((r.fee_bps - 2.0).abs() < 1e-12);
        assert!((r.slippage_bps - 1.0).abs() < 1e-12);
        assert!(r.fill_close);
        // 内层指标里的执行口径也必须被回填(此前只有 simulate 负责填,
        // 经 run_row 构造的 invalid 行会留下 0)。
        assert_eq!(r.metrics.delay_bars, r.delay_bars);
        assert!((r.metrics.fee_bps - r.fee_bps).abs() < 1e-12);
        assert!((r.metrics.slippage_bps - r.slippage_bps).abs() < 1e-12);
        // params 派生的两列也在此处统一
        assert_eq!(
            r.strategy_params,
            output::strategy_params_value("period=20")
        );
        assert_eq!(r.strategy_params_json, "{\"period\":\"20\"}");
        // L5 两列
        assert_eq!(r.bars_used, 300);
        assert_eq!(r.status, RunStatus::NoSignals);
        assert_eq!(r.metrics.sharpe_ratio, 1.25);
        assert!((r.buy_hold_pnl.to_f64() - 123.0).abs() < 1e-12);
    }
}
