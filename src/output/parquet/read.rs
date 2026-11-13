//! 报告/曲线的读取、单文件写入与合并。

use std::path::{Path, PathBuf};

use arrow::record_batch::RecordBatch;

use crate::app::RunResult;
use crate::engine::SummaryMetrics;
use crate::error::Result;

use super::columns::*;
use super::report::write_parquet;
use super::schema::ColumnIndex;

/// 只读 Parquet 元数据里的行数, **不解码任何列**。
///
/// 缓存有效性检查(`run_combo`)只需要"行数是否等于参数数"这一个事实; 原先调用
/// [`read_parquet`] 会把整张表(含全部列、全部行)解码成 `Vec<RunResult>`, 而这条
/// 路径对每个策略都要走一次 —— 参数网格越大、缓存命中越多, 浪费越大。
///
/// 这里只打开 footer 读 `num_rows`; 真正的 schema/内容校验仍由后续的
/// [`read_parquet`] / [`merge_parquet_files`] 在读取数据时完成。
///
/// # Errors
/// 文件打开/footer 解析失败, 或行数超出 `usize` 时返回 `AppError`(crate::error::AppError)。
pub fn parquet_row_count(path: &Path) -> Result<usize> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let rows = builder.metadata().file_metadata().num_rows();
    usize::try_from(rows).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "{}: Parquet 行数 {rows} 无法表示为 usize: {e}",
            path.display()
        ))
    })
}

/// 读取一个组合临时文件/报告文件。
///
/// 兼容策略: `turnover_per_year` 及其后的列都是**历史上逐次追加**的, 旧缓存文件
/// 没有它们。原先用 `batch.num_columns() > 37` 这类**列数**判断, 一旦有人调整列
/// 顺序就会把"有但不同名"的列误判成"没有"。现在统一走 `ColumnIndex::opt`:
/// 按名字找, 找不到才回退。
///
/// # Errors
/// 文件打开/解析失败、缺必需列, 或列类型不符时返回 `AppError`(crate::error::AppError)。
pub fn read_parquet(path: &Path) -> Result<Vec<RunResult>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let ci = ColumnIndex::new(&schema, path);
    let reader = builder.build()?;

    // 历史行为: 缺 params 列直接报错(旧格式无法区分 ssf/ssf_st 等 variant)。
    let params_idx = ci.req("params")?;
    let strategy_idx = ci.req("strategy")?;
    let symbol_idx = ci.req("symbol")?;
    let timeframe_idx = ci.req("timeframe")?;
    let total_return_idx = ci.req("total_return")?;
    let final_equity_idx = ci.req("final_equity")?;
    let max_dd_idx = ci.req("max_drawdown")?;
    let trades_idx = ci.req("trades")?;
    let sharpe_idx = ci.req("sharpe")?;
    let sortino_idx = ci.req("sortino")?;
    let win_rate_idx = ci.req("win_rate")?;
    let pf_idx = ci.req("profit_factor")?;
    let calmar_idx = ci.req("calmar")?;
    let avg_win_idx = ci.req("avg_win")?;
    let avg_loss_idx = ci.req("avg_loss")?;
    let expectancy_idx = ci.req("expectancy")?;
    let cw_idx = ci.req("consecutive_wins")?;
    let cl_idx = ci.req("consecutive_losses")?;
    let volatility_idx = ci.req("volatility_pct")?;
    let recovery_idx = ci.req("recovery_factor")?;
    let avg_hold_idx = ci.req("avg_hold_bars")?;
    let avg_mae_idx = ci.req("avg_mae_price")?;
    let avg_mfe_idx = ci.req("avg_mfe_price")?;
    let edge_idx = ci.req("edge_ratio")?;
    let wl_idx = ci.req("wl_ratio")?;
    let skew_idx = ci.req("skewness")?;
    let kurt_idx = ci.req("kurtosis")?;
    let sqn_idx = ci.req("sqn")?;
    let pf_stab_idx = ci.req("pf_stability")?;
    let z_idx = ci.req("z_score")?;
    let pval_idx = ci.req("p_value")?;
    let fixed_pnl_idx = ci.req("fixed_pnl")?;

    let mut all = Vec::new();
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows();

        let strategy = str_col(&batch, strategy_idx, path)?;
        let symbol = str_col(&batch, symbol_idx, path)?;
        let timeframe = str_col(&batch, timeframe_idx, path)?;
        let params_full = str_col(&batch, params_idx, path)?;
        let pnl = f64_col(&batch, total_return_idx, path)?;
        let eq = dec_col(&batch, final_equity_idx, path)?;
        let mdd = f64_col(&batch, max_dd_idx, path)?;
        let trades = u32_col(&batch, trades_idx, path)?;
        // 旧缓存文件没有止损计数列 → 回退 0(表示"未记录/无止损")。
        let stop_loss_exits = opt_u32(&batch, &ci, "stop_loss_exits", n, path)?;
        let trailing_stop_exits = opt_u32(&batch, &ci, "trailing_stop_exits", n, path)?;
        let strategy_stop_exits = opt_u32(&batch, &ci, "strategy_stop_exits", n, path)?;
        let sharpe = f64_col(&batch, sharpe_idx, path)?;
        let sortino = f64_col(&batch, sortino_idx, path)?;
        let wr = f64_col(&batch, win_rate_idx, path)?;
        let pf = f64_col(&batch, pf_idx, path)?;
        let calmar = f64_col(&batch, calmar_idx, path)?;
        let avg_win = dec_col(&batch, avg_win_idx, path)?;
        let avg_loss = dec_col(&batch, avg_loss_idx, path)?;
        let expectancy = f64_col(&batch, expectancy_idx, path)?;
        let cw = u32_col(&batch, cw_idx, path)?;
        let cl = u32_col(&batch, cl_idx, path)?;
        let volatility = f64_col(&batch, volatility_idx, path)?;
        let recovery = f64_col(&batch, recovery_idx, path)?;
        let avg_hold = f64_col(&batch, avg_hold_idx, path)?;
        let avg_mae = f64_col(&batch, avg_mae_idx, path)?;
        let avg_mfe = f64_col(&batch, avg_mfe_idx, path)?;
        let edge = f64_col(&batch, edge_idx, path)?;
        let wl = f64_col(&batch, wl_idx, path)?;
        let skew = f64_col(&batch, skew_idx, path)?;
        let kurt = f64_col(&batch, kurt_idx, path)?;
        let sqn = f64_col(&batch, sqn_idx, path)?;
        let pf_stab = f64_col(&batch, pf_stab_idx, path)?;
        let z_score = f64_col(&batch, z_idx, path)?;
        let p_value = f64_col(&batch, pval_idx, path)?;
        let fixed_pnl = dec_col(&batch, fixed_pnl_idx, path)?;

        // 以下均为**历史上逐次追加**的可选列, 旧缓存文件没有 → 回退为旧口径。
        let turnover_per_year = opt_f64(&batch, &ci, "turnover_per_year", n, path)?;
        let delay_bars = opt_u32(&batch, &ci, "delay_bars", n, path)?;
        let fee_bps = opt_f64(&batch, &ci, "fee_bps", n, path)?;
        let slippage_bps = opt_f64(&batch, &ci, "slippage_bps", n, path)?;
        let fixed_total_return = opt_f64(&batch, &ci, "fixed_total_return", n, path)?;
        let fixed_sharpe_ratio = opt_f64(&batch, &ci, "fixed_sharpe_ratio", n, path)?;
        let fixed_max_dd = opt_f64(&batch, &ci, "fixed_max_drawdown", n, path)?;
        let fixed_expectancy_bps = opt_f64(&batch, &ci, "fixed_expectancy_bps", n, path)?;
        let fixed_profit_factor = opt_f64(&batch, &ci, "fixed_profit_factor", n, path)?;
        let fill_close = opt_u8(&batch, &ci, "fill_close", n, path)?;
        let buy_hold_pnl = opt_dec(&batch, &ci, "buy_hold_pnl", n, path)?;
        let buy_hold_return = opt_f64(&batch, &ci, "buy_hold_return", n, path)?;
        // O6: 原样读回写入时的编码, 合并时不重算。旧文件无此列 → 0 / 空串。
        let strategy_params = opt_u64(&batch, &ci, "strategy_params", n, path)?;
        let strategy_params_json = match ci.opt("strategy_params_json") {
            Some(i) => str_col(&batch, i, path)?,
            None => vec![String::new(); n],
        };
        // L5: 退化结果标记。旧文件无此列 → Ok / bars_used = 0(表示"未记录")。
        let bars_used = opt_u32(&batch, &ci, "bars_used", n, path)?;
        let status = match ci.opt("status") {
            Some(i) => str_col(&batch, i, path)?
                .iter()
                .map(|s| match s.as_str() {
                    "ok" => crate::app::RunStatus::Ok,
                    "no_signals" => crate::app::RunStatus::NoSignals,
                    "no_trades" => crate::app::RunStatus::NoTrades,
                    // "该档不可计算"必须**如实**还原, 不能落进下面的兜底分支
                    // 变成 `NoSignals` —— 那正是本项目要消除的"不可计算"与
                    // "无信号"混为一谈。
                    "invalid" => crate::app::RunStatus::Invalid,
                    // 破产是"跑出来了但账户归零", 同样不能落进 `NoSignals`.
                    "bankrupt" => crate::app::RunStatus::Bankrupt,
                    // 未知取值(未来版本新增的 status)⇒ 保守落 `NoSignals`:
                    // 未知状态**不是**"算不出来", 不可臆断。
                    _ => crate::app::RunStatus::NoSignals,
                })
                .collect::<Vec<_>>(),
            None => vec![crate::app::RunStatus::default(); n],
        };

        for i in 0..n {
            all.push(RunResult {
                symbol: symbol[i].clone(),
                timeframe: timeframe[i].clone(),
                strategy: strategy[i].clone(),
                params: params_full[i].clone(),
                strategy_params: strategy_params[i],
                strategy_params_json: strategy_params_json[i].clone(),
                delay_bars: delay_bars[i],
                fee_bps: fee_bps[i],
                slippage_bps: slippage_bps[i],
                fill_close: fill_close[i] != 0,
                buy_hold_pnl: buy_hold_pnl[i],
                buy_hold_return: buy_hold_return[i],
                bars_used: bars_used[i],
                status: status[i],
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
                    stop_loss_exits: stop_loss_exits[i],
                    trailing_stop_exits: trailing_stop_exits[i],
                    strategy_stop_exits: strategy_stop_exits[i],
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
                    turnover_per_year: turnover_per_year[i],
                    delay_bars: delay_bars[i],
                    fee_bps: fee_bps[i],
                    slippage_bps: slippage_bps[i],
                    fixed_total_return: fixed_total_return[i],
                    fixed_sharpe_ratio: fixed_sharpe_ratio[i],
                    // 落盘单位是 Ratio(小数), SummaryMetrics 内部是百分数
                    fixed_max_drawdown_pct: fixed_max_dd[i] * 100.0,
                    fixed_expectancy_bps: fixed_expectancy_bps[i],
                    fixed_profit_factor: fixed_profit_factor[i],
                },
            });
        }
    }
    Ok(all)
}

/// 可选 `f64` 列; 旧缓存文件无此列时返回长度 `n` 的默认值。
pub(super) fn opt_f64(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<f64>> {
    match ci.opt(name) {
        Some(i) => f64_col(batch, i, file),
        None => Ok(vec![0.0; n]),
    }
}

/// 可选 `u32` 列。
pub(super) fn opt_u32(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u32>> {
    match ci.opt(name) {
        Some(i) => u32_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 可选 `u64` 列。
pub(super) fn opt_u64(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u64>> {
    match ci.opt(name) {
        Some(i) => u64_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 可选 `u8` 列。
pub(super) fn opt_u8(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u8>> {
    match ci.opt(name) {
        Some(i) => u8_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 把结果行写入单个组合缓存文件(父目录自动创建)。
///
/// # Errors
/// IO 失败时返回 `AppError`(crate::error::AppError)。
pub fn write_single_parquet(
    results: &[RunResult],
    path: &Path,
    balance: crate::money::Amount,
) -> Result<()> {
    write_parquet(path, results, balance)
}

/// 合并本次运行产生的组合文件(含跳过的缓存文件), 输出为最终的 result.parquet。
/// 仅读取显式传入的路径, 避免聚合 `temps/` 中遗留的旧参数文件。
///
/// 合并若干组合临时文件为单一报告。
///
/// **读失败不再静默**: 原实现只打一行日志然后跳过, 返回值仍是成功,
/// 调用方无从得知丢了几个组合 —— 报告里少了参数却显示"完成"。
/// 现在返回 [`MergeStats`], 失败数进入最终摘要; 全部失败才报错。
///
/// # Errors
/// 全部输入文件都读不出来, 或写出合并报告失败时返回 `AppError`(crate::error::AppError)。
pub fn merge_parquet_files(
    paths: &[PathBuf],
    output_path: &Path,
    balance: crate::money::Amount,
) -> Result<MergeStats> {
    let mut all: Vec<RunResult> = Vec::new();
    let mut failed = 0u32;
    for path in paths {
        match read_parquet(path) {
            Ok(mut results) => all.append(&mut results),
            Err(e) => {
                tracing::warn!("  [警告] 读取失败 {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    if all.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "合并失败: {} 个组合文件全部无法读取, 未产出 {}",
            failed,
            output_path.display()
        )));
    }
    let total = all.len() as u64;
    write_parquet(output_path, &all, balance)?;
    Ok(MergeStats {
        merged_rows: total,
        failed_files: failed,
        total_files: u32::try_from(paths.len()).unwrap_or(u32::MAX),
    })
}

/// [`merge_parquet_files`] 的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeStats {
    /// 成功合并的行数。
    pub merged_rows: u64,
    /// 读失败被跳过的组合文件数。
    pub failed_files: u32,
    /// 传入的组合文件总数。
    pub total_files: u32,
}
