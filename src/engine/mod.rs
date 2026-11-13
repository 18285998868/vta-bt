//! 回测引擎: 成交模拟(`simulate` 系列)与全部指标。
//!
//! 子模块按职责拆分:
//!
//! | 模块 | 职责 |
//! |---|---|
//! | `exec` | 执行假设([`ExecConfig`](crate::engine::ExecConfig))与方向([`Side`](crate::engine::Side)) |
//! | `types` | 公开数据类型([`SummaryMetrics`](crate::engine::SummaryMetrics) / [`Trade`](crate::engine::Trade) / [`SimResult`](crate::engine::SimResult)) |
//! | `stops` | 止损配置与止损价计算 |
//! | `sizing` | 仓位模式与开仓数量 |
//! | `scan` | 信号扫描、成交与平仓结算 |
//! | `accum` | 交易/权益曲线的在线累计 |
//! | `metrics` | 指标计算 |

mod accum;
mod exec;
mod metrics;
mod scan;
mod sizing;
mod stops;
mod types;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use accum::{CurveAccum, CurveStats, bar_log_returns};
pub use exec::{ExecConfig, Side};
pub use metrics::buy_hold_pnl;
pub use sizing::SizingConfig;
pub use stops::{AtrStopConfig, BreakevenConfig, StopConfig};
pub use types::{ExitReason, SimResult, SummaryMetrics, Trade};

use rayon::prelude::*;

use crate::strategies::{Signal, StopLines};
use metrics::compute_metrics;

use scan::scan_trades;

/// 执行一次回测并返回全部指标。
///
/// **不物化权益曲线**(曲线统计 O(1))。`--detail` 开关不影响任何指标数值,
/// 详见 `crate::lib` 的不变量 2。
///
/// 注意: 逐笔 pnl 序列仍需保留用于 SQN/偏度/峰度/分半 PF, 最坏 O(交易笔数),
/// 但远小于「每条 bar 两条 f64 曲线」的 O(T) 物化成本。
pub fn simulate(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc(
        close,
        close,
        close,
        open,
        signals,
        initial_cash,
        bars_per_year,
        exec,
        &StopConfig::default(),
    )
}

/// 带盘中 high/low 与持仓期止损的回测。
///
/// `high` / `low` 必须与 `close` 同长(生产路径由 `KlineColumns` 保证); 缺失时
/// 止损判定退回收盘价, 仅用于兼容旧调用。未配置 `risk` 时, `high` / `low`
/// 完全不参与计算, 结果与 [`simulate`] 逐位一致。
///
/// # Errors
/// 执行费率非法、止损百分比越界, 或配置了止损但 `high` / `low` 短于
/// `close` 时返回错误。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc_with_stops(
        high,
        low,
        close,
        open,
        signals,
        None,
        initial_cash,
        bars_per_year,
        exec,
        risk,
    )
}

/// 带策略止损线的盘中回测。
///
/// 与 [`simulate_ohlc`] 相同, 额外接受策略输出的 [`StopLines`]。策略止损线
/// 与引擎 [`StopConfig`] 可以同时存在, 引擎取更紧的一侧; 策略止损触发时
/// 平仓原因记为 [`ExitReason::StrategyStop`]。
///
/// # Errors
/// 执行费率非法、止损参数越界、策略止损线长度不足, 或配置了任一种盘中
/// 止损但 `high` / `low` 短于 `close` 时返回错误。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc_with_stops_and_sizing(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
) -> crate::error::Result<SummaryMetrics> {
    exec.validate_rates()?;
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let needs_ohlc =
        !risk.is_none() || strategy_stops.is_some_and(|s| !s.is_none()) || sizing.needs_ohlc();
    if needs_ohlc && (high.len() < n || low.len() < n) {
        return Err(crate::error::AppError::Msg(format!(
            "配置了止损/ATR 仓位但 high/low 长度不足: high={}, low={}, close={n}",
            high.len(),
            low.len()
        )));
    }
    if n < 2 {
        // 无成交 ⇒ 权益就是初始资金; 执行口径仍需回填, 否则报告里同一列
        // 会出现"数据不足的行是 0、正常行是真实值"的混排。
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(summary);
    }
    // collect_curves = false: 不物化曲线与逐笔 Trade; 曲线统计在线累计,
    // 逐笔 pnl 仍保留(O(交易笔数))用于高阶矩与分半 PF。
    let out = scan_trades(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        &[],
        initial_cash,
        exec,
        risk,
        sizing,
        false,
    )?;
    debug_assert!(
        !out.collect_curves && out.detail.trades.is_empty(),
        "simulate 不得物化落盘产物"
    );
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);
    Ok(summary)
}

/// 带策略止损线的盘中回测(默认全仓)。
///
/// 保留旧签名供既有调用/测试使用; 需要仓位模式时用
/// [`simulate_ohlc_with_stops_and_sizing`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc_with_stops(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc_with_stops_and_sizing(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        initial_cash,
        bars_per_year,
        exec,
        risk,
        &SizingConfig::AllIn,
    )
}

/// 执行一次回测并额外产出逐笔交易与两条曲线。
///
/// 指标经与 [`simulate`] **同一条**路径计算, 二者逐位一致。
pub fn simulate_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc(
        time,
        close,
        close,
        close,
        open,
        signals,
        initial_cash,
        bars_per_year,
        exec,
        &StopConfig::default(),
    )
}

/// 带盘中 high/low 与持仓期止损的详细回测。
///
/// 与 [`simulate_ohlc`] 使用**同一条**指标计算路径, 仅额外物化逐笔交易与曲线。
///
/// # Errors
/// 同 [`simulate_ohlc`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc_with_stops(
        time,
        high,
        low,
        close,
        open,
        signals,
        None,
        initial_cash,
        bars_per_year,
        exec,
        risk,
    )
}

/// 带策略止损线的详细回测。
///
/// 与 [`simulate_detailed_ohlc`] 相同, 额外接受策略输出的 [`StopLines`]。
///
/// # Errors
/// 同 [`simulate_ohlc_with_stops`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc_with_stops_and_sizing(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
) -> crate::error::Result<SimResult> {
    exec.validate_rates()?;
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let needs_ohlc =
        !risk.is_none() || strategy_stops.is_some_and(|s| !s.is_none()) || sizing.needs_ohlc();
    if needs_ohlc && (high.len() < n || low.len() < n) {
        return Err(crate::error::AppError::Msg(format!(
            "配置了止损/ATR 仓位但 high/low 长度不足: high={}, low={}, close={n}",
            high.len(),
            low.len()
        )));
    }
    if n < 2 {
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(SimResult {
            summary,
            trades: Vec::new(),
            equity_curve: Vec::new(),
            fixed_curve: Vec::new(),
        });
    }
    // detailed 路径要落盘, 故额外物化逐笔 Trade 与两条曲线。
    // 注意指标仍只经 `out.metrics` 计算 —— 与 `simulate` 完全同源。
    let out = scan_trades(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        time,
        initial_cash,
        exec,
        risk,
        sizing,
        true,
    )?;
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);

    let detail = out.detail;
    let equity_curve: Vec<(f64, f64)> = detail
        .equity
        .into_iter()
        .enumerate()
        .map(|(i, eq)| {
            let ts = if i < time.len() {
                time[i] as f64
            } else {
                i as f64
            };
            (ts, eq)
        })
        .collect();

    Ok(SimResult {
        summary,
        trades: detail.trades,
        equity_curve,
        fixed_curve: detail.fixed,
    })
}

/// 带策略止损线的详细回测(默认全仓)。
///
/// 保留旧签名供既有调用/测试使用; 需要仓位模式时用
/// [`simulate_detailed_ohlc_with_stops_and_sizing`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc_with_stops(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc_with_stops_and_sizing(
        time,
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        initial_cash,
        bars_per_year,
        exec,
        risk,
        &SizingConfig::AllIn,
    )
}

/// 批量回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch(
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SummaryMetrics>> {
    signal_batch
        .par_iter()
        .map(|signals| simulate(close, open, signals, initial_cash, bars_per_year, exec))
        .collect::<crate::error::Result<Vec<_>>>()
}

/// 批量详细回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SimResult>> {
    signal_batch
        .par_iter()
        .map(|signals| {
            simulate_detailed(
                time,
                close,
                open,
                signals,
                initial_cash,
                bars_per_year,
                exec,
            )
        })
        .collect::<crate::error::Result<Vec<_>>>()
}
