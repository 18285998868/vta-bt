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

fn combo_parquet_path(
    temps_dir: &str,
    symbol: &str,
    tf: &str,
    strategy: &str,
) -> std::path::PathBuf {
    std::path::Path::new(temps_dir).join(format!("{}_{}_{}.parquet", symbol, tf, strategy))
}

pub fn run(config: &Config) -> anyhow::Result<()> {
    let strategies = build_strategies(&config.strategy);
    let detail = config.output.as_ref().is_some_and(|o| o.detail);
    let overwrite = config.output.as_ref().is_some_and(|o| o.overwrite);

    let symbols: Vec<&str> = config.data.symbol.iter().map(|s| s.as_str()).collect();
    let timeframes: Vec<&str> = config.data.timeframes.iter().map(|s| s.as_str()).collect();

    let out_dir = config
        .output
        .as_ref()
        .and_then(|o| o.dir.as_deref())
        .unwrap_or("reports");
    let temps_dir = format!("{out_dir}/temps");

    let mut all_results: Vec<RunResult> = Vec::new();
    let mut all_details: Vec<DetailResult> = Vec::new();
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
                let n = strategy.param_count();
                if n == 0 {
                    continue;
                }

                let combo_path = combo_parquet_path(&temps_dir, symbol, tf, strategy.name());

                if !overwrite && combo_path.exists() {
                    match output::read_parquet(combo_path.to_str().expect("non-UTF-8 path")) {
                        Ok(mut results) => {
                            all_results.append(&mut results);
                            skipped += 1;
                            println!("  [{}]: 已存在, 跳过 ({} 条结果)", strategy.name(), n);
                            continue;
                        }
                        Err(e) => {
                            eprintln!("  [{}] 读取失败: {e}, 重新运行", strategy.name());
                        }
                    }
                }

                use rayon::prelude::*;
                let signal_batch: Vec<Vec<crate::strategies::Signal>> = (0..n)
                    .into_par_iter()
                    .map(|i| strategy.generate_signals(&cols.close, &cols.volume, i))
                    .collect();

                if detail {
                    let sim_results = engine::simulate_batch_detailed(
                        &cols.time,
                        &cols.close,
                        &signal_batch,
                        config.backtest.initial_balance,
                        bpy,
                    );

                    for (i, sim) in sim_results.into_iter().enumerate() {
                        all_results.push(RunResult {
                            symbol: symbol.to_string(),
                            timeframe: tf.to_string(),
                            strategy: strategy.name().to_string(),
                            params: strategy.param_str(i),
                            metrics: sim.summary,
                        });
                        all_details.push(DetailResult {
                            symbol: symbol.to_string(),
                            timeframe: tf.to_string(),
                            strategy: strategy.name().to_string(),
                            params: strategy.param_str(i),
                            trades: sim.trades,
                            equity_curve: sim.equity_curve,
                        });
                    }
                } else {
                    let metrics = engine::simulate_batch(
                        &cols.close,
                        &signal_batch,
                        config.backtest.initial_balance,
                        bpy,
                    );

                    let results: Vec<RunResult> = (0..n)
                        .map(|i| RunResult {
                            symbol: symbol.to_string(),
                            timeframe: tf.to_string(),
                            strategy: strategy.name().to_string(),
                            params: strategy.param_str(i),
                            metrics: metrics[i],
                        })
                        .collect();

                    output::write_single_parquet(
                        &results,
                        &combo_path,
                        config.backtest.initial_balance,
                    )?;
                    all_results.extend(results);
                }

                ran += 1;
            }
        }
    }

    let out_path = format!("{out_dir}/result.parquet");
    output::write_parquet(&out_path, &all_results, config.backtest.initial_balance)?;
    println!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 共 {} 条结果",
        all_results.len()
    );
    println!("  合并 Parquet 已保存: {out_path}");

    if detail && !all_details.is_empty() {
        let detail_dir = format!("{out_dir}/detail");
        output::write_detail_parquet(&all_details, &detail_dir)?;
        println!("  详细数据 Parquet 已保存至: {detail_dir}/");
    }

    Ok(())
}
