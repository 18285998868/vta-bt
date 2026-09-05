use std::path::PathBuf;

use crate::config::Config;
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
fn params_fingerprint(strategy: &dyn crate::strategies::Strategy, n: usize) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
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

/// 执行单个 (symbol, timeframe, strategy) 组合:
/// 1. 若 overwrite=false 且缓存文件(含参数指纹)已存在且可读, 直接返回(跳过)
/// 2. 该策略的 n 个参数并行执行 信号生成+回测; detail 模式收集 trades/equity_curve
/// 3. 计算完成即写入 temps/(及 detail/)
/// 返回 (组合文件路径, 是否本次计算; false 表示命中缓存被跳过)。
fn run_combo(
    strategy: &dyn crate::strategies::Strategy,
    symbol: &str,
    tf: &str,
    cols: &lib_reader::KlineColumns,
    balance: f64,
    bpy: f64,
    detail: bool,
    overwrite: bool,
    temps_dir: &str,
    detail_dir: &str,
) -> crate::error::Result<(PathBuf, bool)> {
    let n = strategy.param_count();

    let combo_path = combo_parquet_path(
        temps_dir,
        symbol,
        tf,
        strategy.name(),
        params_fingerprint(strategy, n),
    );

    if !overwrite && combo_path.exists() {
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

    if detail {
        let sims: Vec<(String, crate::engine::SimResult)> = (0..n)
            .into_par_iter()
            .map(|i| {
                let signals = strategy.generate_signals(&cols.close, &cols.volume, i);
                let sim = crate::engine::simulate_detailed(
                    &cols.time,
                    &cols.close,
                    &signals,
                    balance,
                    bpy,
                );
                (strategy.param_str(i), sim)
            })
            .collect();

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

        output::write_single_parquet(&run_rows, &combo_path, balance)?;
        output::write_detail_parquet(&detail_rows, detail_dir)?;
    } else {
        let run_rows: Vec<RunResult> = (0..n)
            .into_par_iter()
            .map(|i| {
                let signals = strategy.generate_signals(&cols.close, &cols.volume, i);
                let metrics = crate::engine::simulate(&cols.close, &signals, balance, bpy);
                RunResult {
                    symbol: symbol.to_string(),
                    timeframe: tf.to_string(),
                    strategy: strategy.name().to_string(),
                    params: strategy.param_str(i),
                    metrics,
                }
            })
            .collect();

        output::write_single_parquet(&run_rows, &combo_path, balance)?;
    }

    Ok((combo_path, true))
}

/// 整体逻辑编排(与代码实际结构逐行对应):
/// ```
/// 1.  加载配置并校验: config.validate()?; build_strategies → strategies
/// 2.  for symbol in symbols                          // 品种层
/// 3.    for timeframe in timeframes                 // 时间帧层
/// 4.      cols ← load_columns(symbol, timeframe)     // 加载数据: 一次加载供该帧全部策略复用, 帧末释放内存; 失败打印[跳过]并 continue
/// 5.      for strategy in strategies                 // 策略层
/// 6.        run_combo(...) → 单个组合处理, 内部:
/// 7.          a. 跳过判断: 非 overwrite 且组合缓存文件(含参数指纹)可读 → 跳过(仍并入合并)
/// 8.          b. 参数并行: (0..n).into_par_iter 单遍 生成信号 + simulate(/detailed)
/// 9.          c. detail 模式收集 trades/equity_curve
/// 10.         d. 写出: write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
/// 11. merge_parquet_files(combo_files) → result.parquet   // 只合并本次运行组合(新算+跳过), 整文件重建
/// ```
pub fn run(config: &Config) -> crate::error::Result<()> {
    config.validate()?;
    let strategies = build_strategies(&config.strategy);
    let detail = config.output.as_ref().is_some_and(|o| o.detail);
    let overwrite = config.output.as_ref().is_some_and(|o| o.overwrite);
    let balance = config.backtest.initial_balance;

    let symbols: Vec<&str> = config.data.symbol.iter().map(|s| s.as_str()).collect();
    let timeframes: Vec<&str> = config.data.timeframes.iter().map(|s| s.as_str()).collect();

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

    for symbol in &symbols {
        for tf in &timeframes {
            println!("\n{}", "=".repeat(70));
            println!("  {} / {}", symbol, tf);
            println!("{}", "=".repeat(70));

            let cols = match crate::data::load_columns(&config.data, symbol, tf) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("  [跳过] 加载失败: {e}");
                    continue;
                }
            };
            println!("  数据: {} bars\n", cols.len());
            let bpy = bars_per_year(tf);

            for strategy in &strategies {
                let (combo_path, computed) = run_combo(
                    strategy.as_ref(),
                    symbol,
                    tf,
                    &cols,
                    balance,
                    bpy,
                    detail,
                    overwrite,
                    &temps_dir,
                    &detail_dir,
                )?;
                if computed {
                    ran += 1;
                } else {
                    skipped += 1;
                }
                combo_files.push(combo_path);
            }
        }
    }

    let out_path = format!("{out_dir}/result.parquet");
    let merged = output::merge_parquet_files(&combo_files, &out_path, balance)?;
    println!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 合并 {} 条结果",
        merged
    );
    println!("  合并 Parquet 已保存: {out_path}");
    if detail {
        println!("  详细数据 Parquet 已保存至: {detail_dir}/");
    }

    Ok(())
}
