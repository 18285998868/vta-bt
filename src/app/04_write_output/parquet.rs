use std::path::PathBuf;
use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field};
use arrow::record_batch::RecordBatch;

use crate::app::RunResult;
use crate::engine::SummaryMetrics;
use crate::error::Result;

fn params_to_json(params: &str) -> String {
    let clean = params.replace(' ', "");
    if let Some(rest) = clean.strip_prefix("period=") {
        let period = rest.split(',').next().unwrap_or("0");
        if let Some(variant_pos) = rest.find(",variant=") {
            let variant = &rest[variant_pos + 9..];
            format!("{{\"period\":{},\"variant\":\"{}\"}}", period, variant)
        } else {
            format!("{{\"period\":{}}}", period)
        }
    } else {
        // MA strategies: "ema_3", "sma_10", etc. — extract digits after last '_'
        let period = clean
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        format!("{{\"period\":{}}}", period)
    }
}

fn strategy_params_value(params: &str) -> u32 {
    let clean = params.replace(' ', "");
    if let Some(rest) = clean.strip_prefix("period=") {
        rest.split(',')
            .next()
            .unwrap_or("0")
            .parse::<u32>()
            .unwrap_or(0)
    } else {
        // MA strategies: "ema_3", "sma_10", etc.
        clean
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    }
}

fn build_parquet(results: &[RunResult], balance: f64) -> RecordBatch {
    let mut strategy = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut timeframe = StringBuilder::new();
    let mut strategy_params = UInt32Builder::new();
    let mut strategy_params_json = StringBuilder::new();
    let mut params_full = StringBuilder::new();
    let mut bal = Float64Builder::new();
    let mut trade_size = StringBuilder::new();
    let mut pnl = Float64Builder::new();
    let mut eq = Float64Builder::new();
    let mut mdd = Float64Builder::new();
    let mut trades = UInt32Builder::new();
    let mut sharpe = Float64Builder::new();
    let mut sortino = Float64Builder::new();
    let mut wr = Float64Builder::new();
    let mut pf = Float64Builder::new();
    let mut calmar = Float64Builder::new();
    let mut return_mdd = Float64Builder::new();
    let mut avg_win = Float64Builder::new();
    let mut avg_loss = Float64Builder::new();
    let mut expectancy = Float64Builder::new();
    let mut consecutive_wins = UInt32Builder::new();
    let mut consecutive_losses = UInt32Builder::new();
    let mut volatility = Float64Builder::new();
    let mut recovery_factor = Float64Builder::new();
    let mut avg_hold_bars = Float64Builder::new();
    let mut avg_mae = Float64Builder::new();
    let mut avg_mfe = Float64Builder::new();
    let mut edge_ratio = Float64Builder::new();
    let mut wl_ratio = Float64Builder::new();
    let mut skewness = Float64Builder::new();
    let mut kurtosis = Float64Builder::new();
    let mut sqn = Float64Builder::new();
    let mut pf_stability = Float64Builder::new();
    let mut z_score = Float64Builder::new();
    let mut p_value = Float64Builder::new();
    let mut fixed_pnl = Float64Builder::new();

    for r in results {
        let m = &r.metrics;
        strategy.append_value(&r.strategy);
        symbol.append_value(&r.symbol);
        timeframe.append_value(&r.timeframe);
        strategy_params.append_value(strategy_params_value(&r.params));
        strategy_params_json.append_value(params_to_json(&r.params));
        params_full.append_value(&r.params);
        bal.append_value(balance);
        trade_size.append_value("100%");
        pnl.append_value(m.total_return);
        eq.append_value(m.final_equity);
        mdd.append_value(m.max_drawdown_pct / 100.0);
        trades.append_value(m.num_trades);
        sharpe.append_value(m.sharpe_ratio);
        sortino.append_value(m.sortino_ratio);
        wr.append_value(m.win_rate / 100.0);
        pf.append_value(m.profit_factor);
        calmar.append_value(m.calmar_ratio);
        let rmdd = if m.max_drawdown_pct > 0.0 {
            m.total_return_pct / m.max_drawdown_pct
        } else {
            0.0
        };
        return_mdd.append_value(rmdd);
        avg_win.append_value(m.avg_win);
        avg_loss.append_value(m.avg_loss);
        expectancy.append_value(m.expectancy);
        consecutive_wins.append_value(m.consecutive_wins);
        consecutive_losses.append_value(m.consecutive_losses);
        volatility.append_value(m.volatility);
        recovery_factor.append_value(m.recovery_factor);
        avg_hold_bars.append_value(m.avg_hold_bars);
        avg_mae.append_value(m.avg_mae);
        avg_mfe.append_value(m.avg_mfe);
        edge_ratio.append_value(m.edge_ratio);
        wl_ratio.append_value(m.wl_ratio);
        skewness.append_value(m.skewness);
        kurtosis.append_value(m.kurtosis);
        sqn.append_value(m.sqn);
        pf_stability.append_value(m.pf_stability);
        z_score.append_value(m.z_score);
        p_value.append_value(m.p_value);
        fixed_pnl.append_value(m.total_fixed_pnl);
    }

    let cols: Vec<ArrayRef> = vec![
        Arc::new(strategy.finish()),
        Arc::new(symbol.finish()),
        Arc::new(timeframe.finish()),
        Arc::new(strategy_params.finish()),
        Arc::new(strategy_params_json.finish()),
        Arc::new(bal.finish()),
        Arc::new(trade_size.finish()),
        Arc::new(pnl.finish()),
        Arc::new(eq.finish()),
        Arc::new(mdd.finish()),
        Arc::new(trades.finish()),
        Arc::new(sharpe.finish()),
        Arc::new(sortino.finish()),
        Arc::new(wr.finish()),
        Arc::new(pf.finish()),
        Arc::new(calmar.finish()),
        Arc::new(return_mdd.finish()),
        Arc::new(avg_win.finish()),
        Arc::new(avg_loss.finish()),
        Arc::new(expectancy.finish()),
        Arc::new(consecutive_wins.finish()),
        Arc::new(consecutive_losses.finish()),
        Arc::new(volatility.finish()),
        Arc::new(recovery_factor.finish()),
        Arc::new(avg_hold_bars.finish()),
        Arc::new(avg_mae.finish()),
        Arc::new(avg_mfe.finish()),
        Arc::new(edge_ratio.finish()),
        Arc::new(wl_ratio.finish()),
        Arc::new(skewness.finish()),
        Arc::new(kurtosis.finish()),
        Arc::new(sqn.finish()),
        Arc::new(pf_stability.finish()),
        Arc::new(z_score.finish()),
        Arc::new(p_value.finish()),
        Arc::new(fixed_pnl.finish()),
        Arc::new(params_full.finish()),
    ];

    let fields = vec![
        Field::new("strategy", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("timeframe", DataType::Utf8, false),
        Field::new("strategy_params", DataType::UInt32, false),
        Field::new("strategy_params_json", DataType::Utf8, false),
        Field::new("balance", DataType::Float64, false),
        Field::new("trade_size", DataType::Utf8, false),
        Field::new("pnl", DataType::Float64, false),
        Field::new("eq", DataType::Float64, false),
        Field::new("mdd", DataType::Float64, false),
        Field::new("trades", DataType::UInt32, false),
        Field::new("sharpe", DataType::Float64, false),
        Field::new("sortino", DataType::Float64, false),
        Field::new("wr", DataType::Float64, false),
        Field::new("pf", DataType::Float64, false),
        Field::new("calmar", DataType::Float64, false),
        Field::new("return_mdd", DataType::Float64, false),
        Field::new("avg_win", DataType::Float64, false),
        Field::new("avg_loss", DataType::Float64, false),
        Field::new("expectancy", DataType::Float64, false),
        Field::new("consecutive_wins", DataType::UInt32, false),
        Field::new("consecutive_losses", DataType::UInt32, false),
        Field::new("volatility", DataType::Float64, false),
        Field::new("recovery_factor", DataType::Float64, false),
        Field::new("avg_hold_bars", DataType::Float64, false),
        Field::new("avg_mae", DataType::Float64, false),
        Field::new("avg_mfe", DataType::Float64, false),
        Field::new("edge_ratio", DataType::Float64, false),
        Field::new("wl_ratio", DataType::Float64, false),
        Field::new("skewness", DataType::Float64, false),
        Field::new("kurtosis", DataType::Float64, false),
        Field::new("sqn", DataType::Float64, false),
        Field::new("pf_stability", DataType::Float64, false),
        Field::new("z_score", DataType::Float64, false),
        Field::new("p_value", DataType::Float64, false),
        Field::new("fixed_pnl", DataType::Float64, false),
        Field::new("params", DataType::Utf8, false),
    ];

    let schema = arrow::datatypes::Schema::new(fields);
    RecordBatch::try_new(Arc::new(schema), cols).expect("Parquet schema/column count mismatch")
}

pub fn write_parquet(path: &str, results: &[RunResult], balance: f64) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let batch = build_parquet(results, balance);
    let schema = batch.schema();

    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_detail_parquet(details: &[crate::app::DetailResult], dir: &str) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::collections::HashMap;
    use std::fs::File;

    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path)?;

    let mut groups: HashMap<(String, String, String, String), Vec<&crate::app::DetailResult>> =
        HashMap::new();
    for d in details {
        groups
            .entry((
                d.symbol.clone(),
                d.timeframe.clone(),
                d.strategy.clone(),
                d.params.clone(),
            ))
            .or_default()
            .push(d);
    }

    for ((symbol, tf, strategy, params), group) in &groups {
        let clean_params = params.replace(' ', "_").replace('=', "-").replace(',', "_");
        let filename = format!("{}_{}_{}_{}.parquet", symbol, tf, strategy, clean_params);
        let filepath = path.join(filename);

        let mut trade_entry_time = Int64Builder::new();
        let mut trade_exit_time = Int64Builder::new();
        let mut trade_side = StringBuilder::new();
        let mut trade_entry = Float64Builder::new();
        let mut trade_exit = Float64Builder::new();
        let mut trade_qty = Float64Builder::new();
        let mut trade_pnl = Float64Builder::new();
        let mut trade_pnl_pct = Float64Builder::new();
        let mut trade_fixed_pnl = Float64Builder::new();
        let mut trade_bars_held = Int64Builder::new();
        let mut trade_max_runup = Float64Builder::new();
        let mut trade_max_drawdown = Float64Builder::new();
        let mut trade_exit_reason = StringBuilder::new();

        for d in group {
            for t in &d.trades {
                trade_entry_time.append_value(t.entry_time);
                trade_exit_time.append_value(t.exit_time);
                trade_side.append_value(if t.side == 1 { "LONG" } else { "SHORT" });
                trade_entry.append_value(t.entry_price);
                trade_exit.append_value(t.exit_price);
                trade_qty.append_value(t.qty);
                trade_pnl.append_value(t.pnl);
                trade_pnl_pct.append_value(t.pnl_pct);
                trade_fixed_pnl.append_value(t.fixed_pnl);
                trade_bars_held.append_value(t.bars_held);
                trade_max_runup.append_value(t.max_runup);
                trade_max_drawdown.append_value(t.max_drawdown);
                trade_exit_reason.append_value(&t.exit_reason);
            }
        }

        let trade_cols: Vec<ArrayRef> = vec![
            Arc::new(trade_entry_time.finish()),
            Arc::new(trade_exit_time.finish()),
            Arc::new(trade_side.finish()),
            Arc::new(trade_entry.finish()),
            Arc::new(trade_exit.finish()),
            Arc::new(trade_qty.finish()),
            Arc::new(trade_pnl.finish()),
            Arc::new(trade_pnl_pct.finish()),
            Arc::new(trade_fixed_pnl.finish()),
            Arc::new(trade_bars_held.finish()),
            Arc::new(trade_max_runup.finish()),
            Arc::new(trade_max_drawdown.finish()),
            Arc::new(trade_exit_reason.finish()),
        ];

        let trade_fields = vec![
            Field::new("entry_time", DataType::Int64, false),
            Field::new("exit_time", DataType::Int64, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("entry_price", DataType::Float64, false),
            Field::new("exit_price", DataType::Float64, false),
            Field::new("qty", DataType::Float64, false),
            Field::new("pnl", DataType::Float64, false),
            Field::new("pnl_pct", DataType::Float64, false),
            Field::new("fixed_pnl", DataType::Float64, false),
            Field::new("bars_held", DataType::Int64, false),
            Field::new("max_runup", DataType::Float64, false),
            Field::new("max_drawdown", DataType::Float64, false),
            Field::new("exit_reason", DataType::Utf8, false),
        ];

        let trade_schema = arrow::datatypes::Schema::new(trade_fields);
        let trade_batch = RecordBatch::try_new(Arc::new(trade_schema), trade_cols)?;

        if let Some(parent) = filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(&filepath)?;
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, trade_batch.schema(), Some(props))?;
        writer.write(&trade_batch)?;
        writer.close()?;
    }

    Ok(())
}

fn str_col(batch: &RecordBatch, idx: usize) -> Vec<String> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("string column");
    (0..batch.num_rows())
        .map(|i| arr.value(i).to_string())
        .collect()
}

fn f64_col(batch: &RecordBatch, idx: usize) -> Vec<f64> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("f64 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

fn u32_col(batch: &RecordBatch, idx: usize) -> Vec<u32> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("u32 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

pub fn read_parquet(path: &str) -> Result<Vec<RunResult>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    let mut all = Vec::new();
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows();
        if batch.num_columns() < 37 {
            return Err(crate::error::AppError::Msg(format!(
                "缓存文件缺少完整 params 列(旧格式, 无法区分 ssf/ssf_st 等 variant), 请重跑: {path}"
            )));
        }
        let strategy = str_col(&batch, 0);
        let symbol = str_col(&batch, 1);
        let timeframe = str_col(&batch, 2);
        let params_full = str_col(&batch, 36);
        let pnl = f64_col(&batch, 7);
        let eq = f64_col(&batch, 8);
        let mdd = f64_col(&batch, 9);
        let trades = u32_col(&batch, 10);
        let sharpe = f64_col(&batch, 11);
        let sortino = f64_col(&batch, 12);
        let wr = f64_col(&batch, 13);
        let pf = f64_col(&batch, 14);
        let calmar = f64_col(&batch, 15);
        let avg_win = f64_col(&batch, 17);
        let avg_loss = f64_col(&batch, 18);
        let expectancy = f64_col(&batch, 19);
        let cw = u32_col(&batch, 20);
        let cl = u32_col(&batch, 21);
        let volatility = f64_col(&batch, 22);
        let recovery = f64_col(&batch, 23);
        let avg_hold = f64_col(&batch, 24);
        let avg_mae = f64_col(&batch, 25);
        let avg_mfe = f64_col(&batch, 26);
        let edge = f64_col(&batch, 27);
        let wl = f64_col(&batch, 28);
        let skew = f64_col(&batch, 29);
        let kurt = f64_col(&batch, 30);
        let sqn = f64_col(&batch, 31);
        let pf_stab = f64_col(&batch, 32);
        let z_score = f64_col(&batch, 33);
        let p_value = f64_col(&batch, 34);
        let fixed_pnl = f64_col(&batch, 35);

        for i in 0..n {
            all.push(RunResult {
                symbol: symbol[i].clone(),
                timeframe: timeframe[i].clone(),
                strategy: strategy[i].clone(),
                params: params_full[i].clone(),
                metrics: SummaryMetrics {
                    total_return_pct: pnl[i] * 100.0,
                    total_return: pnl[i],
                    sharpe_ratio: sharpe[i],
                    sortino_ratio: sortino[i],
                    max_drawdown_pct: mdd[i] * 100.0,
                    win_rate: wr[i] * 100.0,
                    profit_factor: pf[i],
                    calmar_ratio: calmar[i],
                    num_trades: trades[i],
                    avg_win: avg_win[i],
                    avg_loss: avg_loss[i],
                    expectancy: expectancy[i],
                    consecutive_wins: cw[i],
                    consecutive_losses: cl[i],
                    final_equity: eq[i],
                    volatility: volatility[i],
                    recovery_factor: recovery[i],
                    avg_hold_bars: avg_hold[i],
                    avg_mae: avg_mae[i],
                    avg_mfe: avg_mfe[i],
                    edge_ratio: edge[i],
                    wl_ratio: wl[i],
                    skewness: skew[i],
                    kurtosis: kurt[i],
                    sqn: sqn[i],
                    pf_stability: pf_stab[i],
                    z_score: z_score[i],
                    p_value: p_value[i],
                    total_fixed_pnl: fixed_pnl[i],
                },
            });
        }
    }
    Ok(all)
}

pub fn write_single_parquet(
    results: &[RunResult],
    path: &std::path::Path,
    balance: f64,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_parquet(path.to_str().expect("non-UTF-8 path"), results, balance)
}

/// 合并本次运行产生的组合文件 (含跳过的缓存文件), 输出为最终的 result.parquet。
/// 仅读取显式传入的路径, 避免聚合 `temps/` 中遗留的旧参数文件。
/// 返回合并的总行数。
pub fn merge_parquet_files(paths: &[PathBuf], output_path: &str, balance: f64) -> Result<u64> {
    let mut all: Vec<RunResult> = Vec::new();
    for path in paths {
        match read_parquet(path.to_str().expect("non-UTF-8 path")) {
            Ok(mut results) => all.append(&mut results),
            Err(e) => eprintln!("  [警告] 读取失败 {}: {e}", path.display()),
        }
    }
    let total = all.len() as u64;
    if !all.is_empty() {
        write_parquet(output_path, &all, balance)?;
    }
    Ok(total)
}
