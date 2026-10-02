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
}

#[derive(Debug, Clone)]
pub struct DetailResult {
    pub symbol: String,
    pub timeframe: String,
    pub strategy: String,
    pub params: String,
    pub trades: Vec<engine::Trade>,
    pub equity_curve: Vec<(f64, f64)>,
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
    cols: &lib_reader::KlineColumns,
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

    if ctx.detail && n > 50_000 {
        eprintln!(
            "  [{}] 警告: detail 模式会把 {n} 组参数结果(含逐笔交易/权益曲线)先攒入内存再写出, 可能占用数 GB; 建议减少参数范围或关闭 detail",
            strategy.name()
        );
    }

    if ctx.detail {
        let sims: Vec<(String, crate::engine::SimResult)> = (0..n)
            .into_par_iter()
            .map(|i| {
                let signals =
                    strategy.generate_signals(&cols.open, &cols.close, &cols.volume, i)?;
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

        let mut run_rows = Vec::with_capacity(n);
        let mut detail_rows = Vec::with_capacity(n);
        for (params, sim) in sims {
            let crate::engine::SimResult {
                summary,
                trades,
                equity_curve,
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
            });
            detail_rows.push(DetailResult {
                symbol: symbol.to_string(),
                timeframe: tf.to_string(),
                strategy: strategy.name().to_string(),
                params,
                trades,
                equity_curve,
            });
        }

        output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
        output::write_detail_parquet(&detail_rows, &ctx.detail_dir)?;
    } else {
        let run_rows: Vec<RunResult> = (0..n)
            .into_par_iter()
            .map(|i| {
                let signals =
                    strategy.generate_signals(&cols.open, &cols.close, &cols.volume, i)?;
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
                })
            })
            .collect::<crate::error::Result<Vec<_>>>()?;

        output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
    }

    Ok((combo_path, true))
}

/// 整体逻辑编排(与代码实际结构逐行对应):
/// ```text
/// 1.  加载配置并校验: config::load_config(path)?; config.validate()?; build_strategies → strategies
/// 2.  for symbol in data.symbols()                        // 品种层(单值时长度为 1)
/// 3.    for tf in data.timeframes()                       // 周期层(单值时长度为 1)
/// 4.      cols ← lib_reader::load_columns(data.for_pair(symbol, tf))   // 逐对加载
/// 5.      for strategy in strategies                      // 策略层
/// 6.        run_combo(...) → 单个组合处理, 内部:
/// 7.          a. 跳过判断: 非 overwrite 且组合缓存文件(含参数指纹)可读 → 跳过(仍并入合并)
/// 8.          b. 参数并行: (0..n).into_par_iter 单遍 生成信号 + simulate(/detailed)
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

            let pair_config = config.data.for_pair(symbol, tf);
            let cols = match lib_reader::load_columns(&pair_config) {
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
