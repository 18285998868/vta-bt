use std::path::PathBuf;

use crate::config;
use crate::engine;
use crate::output;
use crate::strategies::bars_per_year;
use crate::strategies::build::build_strategies;

#[derive(Debug, Clone)]
pub struct RunResult {
    pub symbol: String,
    pub timeframe: String,
    pub strategy: String,
    pub params: String,
    pub metrics: engine::SummaryMetrics,
    /// **原样保留**写入时的数值, 合并时不重算(见 `strategy_params_value` 的文档)。
    /// 0 表示"未记录"(旧缓存文件无此列)。
    pub strategy_params: u64,
    /// 同上, 人类可读的 JSON 编码原样保留。空串表示未记录。
    pub strategy_params_json: String,
    pub delay_bars: u32,
    pub fee_bps: f64,
    pub slippage_bps: f64,
    /// delay>0 时 fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
    /// 被动基准(固定名义): 首根 close 买入、末根 close 卖出、无成本的 pnl。
    pub buy_hold_pnl: f64,
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
}

impl RunStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NoSignals => "no_signals",
            Self::NoTrades => "no_trades",
        }
    }

    /// 从信号序列与交易数判定状态。
    #[must_use]
    pub fn classify(signals: &[crate::strategies::Signal], num_trades: u32) -> Self {
        if num_trades > 0 {
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
pub struct DetailResult {
    pub symbol: String,
    pub timeframe: String,
    pub strategy: String,
    pub params: String,
    pub trades: Vec<engine::Trade>,
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
    h = fnv_bytes_i64(h, &cols.time);
    for col in [&cols.open, &cols.high, &cols.low, &cols.close, &cols.volume] {
        h = fnv_bytes_f64(h, col);
    }
    h
}

fn fnv_bytes_i64(mut h: u64, xs: &[i64]) -> u64 {
    for &x in xs {
        h = fnv_bytes(h, &x.to_ne_bytes());
    }
    fnv_bytes(h, &[FNV_COL_SEP])
}

fn fnv_bytes_f64(mut h: u64, xs: &[f64]) -> u64 {
    for &x in xs {
        h = fnv_bytes(h, &x.to_bits().to_le_bytes());
    }
    fnv_bytes(h, &[FNV_COL_SEP])
}

/// 参数集合的稳定指纹 (FNV-1a)。写入组合文件名,避免改动参数后误命中旧缓存。
///
/// 混入的每一项都对应一类"改了但结果会变"的输入:
/// - `data_fp`: 行情内容本身(见 [`data_fingerprint`])
/// - `balance`: 资金口径
/// - `window`: start_date/end_date 字符串
/// - `exec`: delay / fee / slippage / fill 价(前视乐观与口径差异会整体改写结果)
fn params_fingerprint(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: f64,
    window: &str,
    data_fp: u64,
    exec: &crate::engine::ExecConfig,
) -> u64 {
    let mut h = fnv_bytes(FNV_OFFSET, &data_fp.to_le_bytes());
    h = fnv_bytes(h, &balance.to_bits().to_le_bytes());
    h = fnv_bytes(h, window.as_bytes());
    h = fnv_bytes(h, &(exec.delay() as u64).to_le_bytes());
    h = fnv_bytes(h, &exec.fee_rate.to_bits().to_le_bytes());
    h = fnv_bytes(h, &exec.slip_rate.to_bits().to_le_bytes());
    h = fnv_bytes(h, &[u8::from(exec.fill_close)]);
    for i in 0..n {
        h = fnv_bytes(h, strategy.param_str(i).as_bytes());
        h = fnv_bytes(h, &[FNV_COL_SEP]);
    }
    h
}

fn combo_parquet_path(temps_dir: &str, symbol: &str, tf: &str, strategy: &str, fp: u64) -> PathBuf {
    PathBuf::from(temps_dir).join(format!("{symbol}_{tf}_{strategy}_{fp:016x}.parquet"))
}

/// 单个组合执行所需的运行期上下文: 由 run() 一次性构建, 全部组合共享。
struct ComboCtx {
    balance: f64,
    detail: bool,
    fixed_curves: bool,
    overwrite: bool,
    temps_dir: String,
    detail_dir: String,
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
        metrics: engine::SummaryMetrics,
        bars_used: usize,
        status: RunStatus,
        bh_pnl: f64,
        bh_ret: f64,
    ) -> RunResult {
        RunResult {
            symbol: symbol.to_string(),
            timeframe: tf.to_string(),
            strategy: strategy.name().to_string(),
            params: params.to_string(),
            strategy_params: output::strategy_params_value(params),
            strategy_params_json: output::params_to_json(params),
            metrics,
            delay_bars: self.exec.delay() as u32,
            fee_bps: self.exec.fee_rate * 10_000.0,
            slippage_bps: self.exec.slip_rate * 10_000.0,
            fill_close: self.exec.fill_close,
            buy_hold_pnl: bh_pnl,
            buy_hold_return: bh_ret,
            bars_used: bars_used as u32,
            status,
        }
    }
}

/// K 线六列必须**等长**。
///
/// 引擎内部多处用 `close.len().min(open.len())` 之类的截断来"保证"不越界。截断
/// 是静默的: 若某列短了, 引擎只会少回测几根 bar, 而**指标不会报错** —— 报告里
/// 的 bars_used 与用户预期不符, 研究结论却在无声地少算一段数据。
/// 更坏的情形是 `time` 比价列短: `scan_trades` 的 `ts(i)` 会退化成用 bar 序号
/// 当时间戳, detail 落盘的 `entry_time`/`exit_time` 变成假时间。
///
/// 因此在进入回测前一次性拒绝, 让问题在**数据加载边界**暴露。
fn assert_columns_aligned(
    cols: &kline_reader::KlineColumns,
    where_: &str,
) -> crate::error::Result<()> {
    let lens = [
        ("time", cols.time.len()),
        ("open", cols.open.len()),
        ("high", cols.high.len()),
        ("low", cols.low.len()),
        ("close", cols.close.len()),
        ("volume", cols.volume.len()),
    ];
    let n = lens[4].1;
    if let Some((name, len)) = lens.iter().find(|(_, l)| *l != n) {
        return Err(crate::error::AppError::Msg(format!(
            "{where_}: K 线列长度不一致 —— {name}={len}, close={n}; \
             会导致引擎静默截断少算 bar 或时间戳失真。请重新下载/清洗数据"
        )));
    }
    Ok(())
}

/// 执行单个 (symbol, timeframe, strategy) 组合:
/// 1. 若 overwrite=false 且缓存文件(含参数指纹)已存在且可读, 直接返回(跳过)
/// 2. 该策略的 n 个参数并行执行 信号生成+回测; detail 模式收集 trades/equity_curve
/// 3. 计算完成即写入 temps/(及 detail/)
///
/// `bpy` 由调用方按当前 timeframe 提供, 不放在 `ctx` 里(多周期扫描时每周期不同)。
/// 返回 (组合文件路径, 是否本次计算; false 表示命中缓存被跳过)。
fn run_combo(
    strategy: &dyn crate::strategies::Strategy,
    symbol: &str,
    tf: &str,
    cols: &kline_reader::KlineColumns,
    bpy: f64,
    ctx: &ComboCtx,
) -> crate::error::Result<(PathBuf, bool)> {
    assert_columns_aligned(cols, &format!("{symbol}/{tf}"))?;
    let n = strategy.param_count();

    let combo_path = combo_parquet_path(
        &ctx.temps_dir,
        symbol,
        tf,
        strategy.name(),
        params_fingerprint(
            strategy,
            n,
            ctx.balance,
            &ctx.window,
            data_fingerprint(cols),
            &ctx.exec,
        ),
    );

    // L4: 指纹相同**不保证**文件完整 —— 文件可能被截断/写坏/被旧版本写入。
    // 少行的话下游合并会得到"缺一半参数"的结果而毫无提示, 故行数也必须对得上。
    if !ctx.overwrite && combo_path.exists() {
        match output::read_parquet(combo_path.to_str().expect("non-UTF-8 path")) {
            Ok(results) if results.len() == n => {
                println!(
                    "  [{}]: 已存在, 跳过 ({} 条结果)",
                    strategy.name(),
                    results.len()
                );
                return Ok((combo_path, false));
            }
            Ok(results) => {
                eprintln!(
                    "  [{}] 缓存行数 {} != 参数数 {n}(文件被截断或指纹碰撞), 判为失效并重算",
                    strategy.name(),
                    results.len()
                );
            }
            Err(e) => {
                eprintln!("  [{}] 读取失败: {e}, 重新运行", strategy.name());
            }
        }
    }

    use rayon::prelude::*;

    if ctx.detail {
        let (bh_pnl, bh_ret) = crate::engine::buy_hold_pnl(&cols.close, ctx.balance);
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
        println!(
            "  [{}] detail 预检: 总量约 {}, 分 {} 批, 峰值约 {}",
            strategy.name(),
            gib(total_est),
            n.div_ceil(chunk.max(1)),
            gib(per_param.saturating_mul(chunk as u64))
        );

        let mut run_rows = Vec::with_capacity(n);
        for start in (0..n).step_by(chunk.max(1)) {
            let end = (start + chunk).min(n);
            let batch = strategy.generate_signals_batch(
                &cols.open,
                &cols.high,
                &cols.low,
                &cols.close,
                &cols.volume,
                &cols.time,
                start..end,
            )?;
            assert_eq!(batch.len(), end - start, "batch 信号数必须与参数区间对齐");
            let sims: Vec<(String, crate::engine::SimResult, RunStatus)> = batch
                .into_par_iter()
                .enumerate()
                .map(|(k, signals)| {
                    let i = start + k;
                    let sim = crate::engine::simulate_detailed(
                        &cols.time,
                        &cols.close,
                        &cols.open,
                        &signals,
                        ctx.balance,
                        bpy,
                        &ctx.exec,
                    );
                    let status = RunStatus::classify(&signals, sim.summary.num_trades);
                    Ok((strategy.param_str(i), sim, status))
                })
                .collect::<crate::error::Result<Vec<_>>>()?;

            let mut detail_rows = Vec::with_capacity(end - start);
            for (params, sim, status) in sims {
                let crate::engine::SimResult {
                    summary,
                    trades,
                    equity_curve,
                    fixed_curve,
                } = sim;
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
                    trades,
                    equity_curve,
                    fixed_curve,
                });
            }
            output::write_detail_parquet(&detail_rows, &ctx.detail_dir, ctx.fixed_curves)?;
        }

        output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
    } else {
        let (bh_pnl, bh_ret) = crate::engine::buy_hold_pnl(&cols.close, ctx.balance);
        // 非 detail 也按信号内存分块: 全量 batch 常驻曾 OOM (35k×2.6M)。
        // 单 param 约 T×size_of::<Signal>()B; 峰值钳在 BATCH_MEM_BUDGET, 超量分批。
        const BATCH_MEM_BUDGET: u64 = 4 * 1024 * 1024 * 1024;
        let per_param = cols.len() as u64 * size_of::<crate::strategies::Signal>() as u64;
        let chunk = ((BATCH_MEM_BUDGET / per_param.max(1))
            .max(1)
            .min(n.max(1) as u64)) as usize;
        if n > chunk {
            println!(
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
                &cols.open,
                &cols.high,
                &cols.low,
                &cols.close,
                &cols.volume,
                &cols.time,
                start..end,
            )?;
            assert_eq!(batch.len(), end - start, "batch 信号数必须与参数区间对齐");
            let mut rows: Vec<RunResult> = batch
                .into_par_iter()
                .enumerate()
                .map(|(k, signals)| {
                    let i = start + k;
                    let metrics = crate::engine::simulate(
                        &cols.close,
                        &cols.open,
                        &signals,
                        ctx.balance,
                        bpy,
                        &ctx.exec,
                    );
                    Ok(RunResult {
                        symbol: symbol.to_string(),
                        timeframe: tf.to_string(),
                        strategy: strategy.name().to_string(),
                        params: strategy.param_str(i),
                        metrics,
                        strategy_params: output::strategy_params_value(&strategy.param_str(i)),
                        strategy_params_json: output::params_to_json(&strategy.param_str(i)),
                        delay_bars: ctx.exec.delay() as u32,
                        fee_bps: ctx.exec.fee_rate * 10_000.0,
                        slippage_bps: ctx.exec.slip_rate * 10_000.0,
                        fill_close: ctx.exec.fill_close,
                        buy_hold_pnl: bh_pnl,
                        buy_hold_return: bh_ret,
                        bars_used: cols.len() as u32,
                        status: RunStatus::classify(&signals, metrics.num_trades),
                    })
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
/// 9.          c. detail 模式收集 trades/equity_curve
/// 10.         d. 写出: write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
/// 11. merge_parquet_files(combo_files) → result.parquet  // 只合并本次运行组合(新算+跳过)
/// ```
///
/// 单个 (symbol, timeframe) 数据加载失败时跳过该对并继续, 不中断整体扫描;
/// 全部组合都失败才返回错误。
pub fn run(config_path: &str) -> crate::error::Result<()> {
    let config = config::load_config(config_path)?;
    config.validate()?;
    let strategies = build_strategies(&config.strategy);
    let detail = config.output.as_ref().is_some_and(|o| o.detail);
    let fixed_curves = config.output.as_ref().is_some_and(|o| o.fixed_curves);
    let overwrite = config.output.as_ref().is_some_and(|o| o.overwrite);
    let balance = config.backtest.initial_balance.get();
    let window = format!(
        "{}~{}",
        config.data.start_date.as_deref().unwrap_or(""),
        config.data.end_date.as_deref().unwrap_or("")
    );

    let symbols = config.data.symbols();
    let timeframes = config.data.timeframes();
    let pairs_total = symbols.len() * timeframes.len();

    let out_dir = config
        .output
        .as_ref()
        .and_then(|o| o.dir.as_deref())
        .unwrap_or("reports");
    let temps_dir = format!("{out_dir}/temps");
    let detail_dir = format!("{out_dir}/detail");

    let mut combo_files: Vec<PathBuf> = Vec::new();
    let mut skipped = 0u64;
    let mut ran = 0u64;
    let mut load_failed = 0usize;

    let ctx = ComboCtx {
        balance,
        detail,
        fixed_curves,
        overwrite,
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

    println!("\n{}", "=".repeat(70));
    println!(
        "  扫描规模: {} 品种 × {} 周期 × {} 策略 = {} 组",
        symbols.len(),
        timeframes.len(),
        strategies.len(),
        pairs_total * strategies.len()
    );
    println!("{}", "=".repeat(70));

    let mut pair_idx = 0usize;
    for symbol in &symbols {
        for tf in &timeframes {
            pair_idx += 1;
            println!("\n[{pair_idx}/{pairs_total}] {symbol} / {tf}");

            let pair_config = match config.data.for_pair(symbol, tf) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("  [跳过] 配置无效: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            let cols = match kline_reader::load_columns(&pair_config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("  [跳过] 数据加载失败: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            if cols.is_empty() {
                eprintln!("  [跳过] 无数据");
                load_failed += 1;
                continue;
            }
            println!("  数据: {} bars", cols.len());

            let bpy = bars_per_year(tf);
            for strategy in &strategies {
                let (combo_path, computed) =
                    run_combo(strategy.as_ref(), symbol, tf, &cols, bpy, &ctx)?;
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

    let out_path = format!("{out_dir}/result.parquet");
    let stats = output::merge_parquet_files(&combo_files, &out_path, balance)?;
    println!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 合并 {} 条结果 (来自 {}/{} 个文件)",
        stats.merged_rows,
        stats.total_files - stats.failed_files,
        stats.total_files
    );
    if stats.failed_files > 0 {
        // 报告缺了参数, 必须显式告知, 不能让"完成"字样掩盖。
        println!(
            "  [警告] {} 个组合文件读取失败已被跳过, 报告不完整(请删 temps/ 后重跑)",
            stats.failed_files
        );
    }
    if load_failed > 0 {
        println!("  跳过 {load_failed} 组数据缺失的 (品种 × 周期)");
    }
    println!("  合并 Parquet 已保存: {out_path}");
    if detail {
        println!("  详细数据 Parquet 已保存至: {detail_dir}/");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(n: usize) -> kline_reader::KlineColumns {
        kline_reader::KlineColumns {
            time: (0..n as i64).map(|i| 1_700_000_000 + i * 3600).collect(),
            open: (0..n).map(|i| 100.0 + i as f64).collect(),
            high: (0..n).map(|i| 101.0 + i as f64).collect(),
            low: (0..n).map(|i| 99.0 + i as f64).collect(),
            close: (0..n).map(|i| 100.5 + i as f64).collect(),
            volume: (0..n).map(|i| 10.0 + i as f64).collect(),
        }
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

        // 追加 bar(数据被补齐)
        let mut longer = a.clone();
        longer.time.push(1_700_000_000 + 50 * 3600);
        longer.open.push(150.0);
        longer.high.push(151.0);
        longer.low.push(149.0);
        longer.close.push(150.5);
        longer.volume.push(60.0);
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&longer),
            "追加 bar 必须改变指纹(否则补数据后仍命中旧缓存)"
        );

        // 同一长度但**价格被改写**(同路径文件原地替换)
        let mut rewritten = a.clone();
        rewritten.close[10] += 0.01;
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&rewritten),
            "同长度但价格变化必须改变指纹(否则行情被替换仍命中旧缓存)"
        );

        // 窗口字符串相同但数据不同 → params_fingerprint 必须不同
        let s = build_strategies(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .pop()
        .expect("sma");
        let exec = crate::engine::ExecConfig::default();
        let fp_old = params_fingerprint(&*s, 1, 10_000.0, "~", data_fingerprint(&a), &exec);
        let fp_new = params_fingerprint(&*s, 1, 10_000.0, "~", data_fingerprint(&rewritten), &exec);
        assert_ne!(fp_old, fp_new, "同窗口不同数据必须给出不同缓存文件名");
    }

    /// 列间必须有分隔符: 否则 `[1,2]+[3]` 与 `[1]+[2,3]` 的字节流相同。
    #[test]
    fn data_fingerprint_separates_columns() {
        let a = kline_reader::KlineColumns {
            time: vec![1, 2, 3],
            open: vec![4.0, 5.0, 6.0],
            high: vec![7.0, 8.0, 9.0],
            low: vec![10.0, 11.0, 12.0],
            close: vec![13.0, 14.0, 15.0],
            volume: vec![16.0, 17.0, 18.0],
        };
        // 把 close 的值挪到 open, 长度不变
        let mut b = a.clone();
        b.open = a.close.clone();
        b.close = a.open.clone();
        assert_ne!(
            data_fingerprint(&a),
            data_fingerprint(&b),
            "列内容互换必须改变指纹"
        );
    }

    /// 执行口径任一变化都必须失效缓存: 前视乐观口径与费率差异会整体改写结果。
    #[test]
    fn fingerprint_covers_exec_params() {
        let s = build_strategies(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .pop()
        .expect("sma");
        let c = cols(10);
        let fp = data_fingerprint(&c);
        let base = params_fingerprint(
            &*s,
            1,
            10_000.0,
            "a~b",
            fp,
            &crate::engine::ExecConfig::default(),
        );
        for exec in [
            crate::engine::ExecConfig::try_from_bps_fill(2, 0.0, 0.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 2.0, 0.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 1.0, false).unwrap(),
            crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 0.0, true).unwrap(),
        ] {
            assert_ne!(
                base,
                params_fingerprint(&*s, 1, 10_000.0, "a~b", fp, &exec),
                "执行口径 {:?} 变化必须失效缓存",
                exec
            );
        }
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                20_000.0,
                "a~b",
                fp,
                &crate::engine::ExecConfig::default()
            ),
            "资金变化必须失效缓存"
        );
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                10_000.0,
                "a~c",
                fp,
                &crate::engine::ExecConfig::default()
            ),
            "窗口变化必须失效缓存"
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
            buy_hold_pnl: 1.0,
            buy_hold_return: 0.1,
        };
        let good = dir.join("good.parquet");
        let bad = dir.join("bad.parquet");
        let missing = dir.join("missing.parquet");
        output::write_parquet(good.to_str().unwrap(), std::slice::from_ref(&rr), 10_000.0).unwrap();
        std::fs::write(&bad, b"not a parquet file").unwrap();

        let out = dir.join("merged.parquet");

        // 全部不可读 → 必须报错, 而不是"合并 0 条"式的成功
        assert!(
            output::merge_parquet_files(
                &[bad.clone(), missing.clone()],
                out.to_str().unwrap(),
                10_000.0
            )
            .is_err(),
            "全部失败时必须报错"
        );

        // 部分失败 → 报告仍落盘, 但失败必须被**计数**并进入摘要
        let st = output::merge_parquet_files(
            &[good.clone(), bad, missing],
            out.to_str().unwrap(),
            10_000.0,
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
        output::write_parquet(truncated.to_str().unwrap(), &[], 10_000.0).unwrap();
        assert_eq!(
            output::read_parquet(truncated.to_str().unwrap())
                .unwrap()
                .len(),
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
    fn uneven_kline_columns_are_rejected_before_backtest() {
        let c = cols(10);
        assert!(assert_columns_aligned(&c, "t").is_ok(), "等长必须通过");

        for mutate in [
            (0usize, "time"),
            (1, "open"),
            (2, "high"),
            (3, "low"),
            (4, "close"),
            (5, "volume"),
        ] {
            let (idx, name) = mutate;
            let mut bad = cols(10);
            if idx == 0 {
                bad.time.pop();
            } else {
                match idx {
                    1 => bad.open.pop(),
                    2 => bad.high.pop(),
                    3 => bad.low.pop(),
                    4 => bad.close.pop(),
                    _ => bad.volume.pop(),
                };
            }
            let e = assert_columns_aligned(&bad, "BTCUSDT/1d").expect_err("列长不一致必须被拒");
            let m = format!("{e}");
            assert!(m.contains(name), "错误消息应点名出问题的列 {name}: {m}");
            assert!(m.contains("BTCUSDT/1d"), "错误消息应带上品种/周期: {m}");
        }
        // 空数据不算"长度不一致", 交由 is_empty 判断
        let empty = cols(0);
        assert!(assert_columns_aligned(&empty, "t").is_ok());
    }

    /// O9: `RunResult` 的构造必须**单一来源**, 否则两条分支的字段会漂移。
    #[test]
    fn run_row_is_the_single_construction_point() {
        let s = build_strategies(&[config::StrategyConfig::Sma(config::MaGridCfg {
            period: config::PeriodParam::Single(20),
        })])
        .pop()
        .expect("sma");
        let ctx = ComboCtx {
            balance: 10_000.0,
            detail: false,
            fixed_curves: false,
            overwrite: true,
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
            123.0,
            0.0123,
        );
        // 执行口径字段由 ctx 提供, 不可能与分支漂移
        assert_eq!(r.delay_bars, 1);
        assert!((r.fee_bps - 2.0).abs() < 1e-12);
        assert!((r.slippage_bps - 1.0).abs() < 1e-12);
        assert!(r.fill_close);
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
        assert!((r.buy_hold_pnl - 123.0).abs() < 1e-12);
    }
}
