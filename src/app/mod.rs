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
    pub delay_bars: u32,
    pub fee_bps: f64,
    pub slippage_bps: f64,
    /// delay>0 时 fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
    /// 被动基准(固定名义): 首根 close 买入、末根 close 卖出、无成本的 pnl。
    pub buy_hold_pnl: f64,
    /// 被动基准总收益(=pnl/名义)。
    pub buy_hold_return: f64,
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

/// 参数集合的稳定指纹 (FNV-1a)。写入组合文件名,避免改动参数后误命中旧缓存。
/// 除策略参数外还混入初始资金与数据窗口,防止改 balance / start_date 后仍命中旧临时文件
/// (否则缓存结果的指标与当前资金口径不一致)。
fn params_fingerprint(
    strategy: &dyn crate::strategies::Strategy,
    n: usize,
    balance: f64,
    window: &str,
    exec: &crate::engine::ExecConfig,
) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in balance.to_bits().to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in window.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // 执行口径变更必须失效旧缓存, 否则 delay/费率/fill 不同的结果会误命中。
    for b in (exec.delay_bars as u64).to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in exec.fee_rate.to_bits().to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in exec.slip_rate.to_bits().to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= u64::from(exec.fill_close);
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    for i in 0..n {
        for b in strategy.param_str(i).bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
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
    let n = strategy.param_count();

    let combo_path = combo_parquet_path(
        &ctx.temps_dir,
        symbol,
        tf,
        strategy.name(),
        params_fingerprint(strategy, n, ctx.balance, &ctx.window, &ctx.exec),
    );

    if !ctx.overwrite && combo_path.exists() {
        match output::read_parquet(combo_path.to_str().expect("non-UTF-8 path")) {
            Ok(results) => {
                println!(
                    "  [{}]: 已存在, 跳过 ({} 条结果)",
                    strategy.name(),
                    results.len()
                );
                return Ok((combo_path, false));
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
            let sims: Vec<(String, crate::engine::SimResult)> = batch
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
                    Ok((strategy.param_str(i), sim))
                })
                .collect::<crate::error::Result<Vec<_>>>()?;

            let mut detail_rows = Vec::with_capacity(end - start);
            for (params, sim) in sims {
                let crate::engine::SimResult {
                    summary,
                    trades,
                    equity_curve,
                    fixed_curve,
                } = sim;
                run_rows.push(RunResult {
                    symbol: symbol.to_string(),
                    timeframe: tf.to_string(),
                    strategy: strategy.name().to_string(),
                    params: params.clone(),
                    metrics: summary,
                    delay_bars: ctx.exec.delay_bars as u32,
                    fee_bps: ctx.exec.fee_rate * 10_000.0,
                    slippage_bps: ctx.exec.slip_rate * 10_000.0,
                    fill_close: ctx.exec.fill_close,
                    buy_hold_pnl: bh_pnl,
                    buy_hold_return: bh_ret,
                });
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
                        delay_bars: ctx.exec.delay_bars as u32,
                        fee_bps: ctx.exec.fee_rate * 10_000.0,
                        slippage_bps: ctx.exec.slip_rate * 10_000.0,
                        fill_close: ctx.exec.fill_close,
                        buy_hold_pnl: bh_pnl,
                        buy_hold_return: bh_ret,
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
    let balance = config.backtest.initial_balance;
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
        exec: crate::engine::ExecConfig::from_bps_fill(
            config.backtest.delay_bars,
            config.backtest.fee_bps,
            config.backtest.slippage_bps,
            config.backtest.fill_bar_price == config::FillBarPrice::Close,
        ),
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
    let merged = output::merge_parquet_files(&combo_files, &out_path, balance)?;
    println!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 合并 {} 条结果",
        merged
    );
    if load_failed > 0 {
        println!("  跳过 {load_failed} 组数据缺失的 (品种 × 周期)");
    }
    println!("  合并 Parquet 已保存: {out_path}");
    if detail {
        println!("  详细数据 Parquet 已保存至: {detail_dir}/");
    }

    Ok(())
}
