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

use crate::strategies::Signal;
use metrics::compute_metrics;

use scan::scan_trades;

/// OHLC 行情视图(引擎不读 volume)。
///
/// 字段与构造顺序统一为 `open, high, low, close`; 旧 API 的
/// `(high, low, close, open)` 顺序由兼容包装层负责转换。
#[derive(Debug, Clone, Copy)]
pub struct Ohlcv<'a> {
    /// 开盘价。
    pub open: &'a [f64],
    /// 最高价。
    pub high: &'a [f64],
    /// 最低价。
    pub low: &'a [f64],
    /// 收盘价。
    pub close: &'a [f64],
}

impl<'a> Ohlcv<'a> {
    /// 完整 OHLC 视图。
    #[must_use]
    pub const fn new(open: &'a [f64], high: &'a [f64], low: &'a [f64], close: &'a [f64]) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }

    /// 仅收盘价序列: `high`/`low` 退化为 `close`, 不参与任何计算。
    #[must_use]
    pub const fn close_only(close: &'a [f64], open: &'a [f64]) -> Self {
        Self {
            open,
            high: close,
            low: close,
            close,
        }
    }
}

/// 一次模拟的运行参数(行情之外的输入)。
#[derive(Debug, Clone, Copy)]
pub struct SimOptions<'a> {
    /// 初始资金。
    pub initial_cash: crate::money::Amount,
    /// 年化 bar 数。
    pub bars_per_year: f64,
    /// 执行假设(延迟/费率/滑点/成交价)。
    pub exec: &'a ExecConfig,
    /// 引擎持仓期风险规则。
    pub risk: &'a StopConfig,
    /// 仓位模式。
    pub sizing: &'a SizingConfig,
}

impl<'a> SimOptions<'a> {
    /// 默认口径: 无止损、全仓。
    #[must_use]
    pub const fn new(
        initial_cash: crate::money::Amount,
        bars_per_year: f64,
        exec: &'a ExecConfig,
    ) -> Self {
        Self {
            initial_cash,
            bars_per_year,
            exec,
            risk: &StopConfig::NONE,
            sizing: &SizingConfig::AllIn,
        }
    }
}

/// 两个主入口共用的输入校验。
///
/// 顺序固定为: 执行口径 → 风险规则 → 仓位模式 → 盘中列长度。校验通过后返回
/// 实际参与回测的 bar 数 `min(close.len(), signals.len())`。
fn validate_inputs(
    ohlcv: Ohlcv<'_>,
    signals: &[Signal],
    opts: &SimOptions<'_>,
) -> crate::error::Result<usize> {
    opts.exec.validate_rates()?;
    opts.risk.validate()?;
    opts.sizing.validate()?;
    let n = ohlcv.close.len().min(signals.len());
    let needs_ohlc = !opts.risk.is_none() || opts.sizing.needs_ohlc();
    if needs_ohlc && (ohlcv.high.len() < n || ohlcv.low.len() < n) {
        return Err(crate::error::AppError::Msg(format!(
            "配置了止损/ATR 仓位但 high/low 长度不足: high={}, low={}, close={n}",
            ohlcv.high.len(),
            ohlcv.low.len()
        )));
    }
    Ok(n)
}

/// 数据不足(< 2 bar)时的空指标: 权益 = 初始资金, 执行口径照常回填。
///
/// 若不回填, 报告里同一列会出现"数据不足的行是 0、正常行是真实值"的混排。
fn empty_summary(opts: &SimOptions<'_>) -> SummaryMetrics {
    let mut summary = SummaryMetrics {
        final_equity: opts.initial_cash,
        ..SummaryMetrics::default()
    };
    summary.apply_exec(opts.exec);
    summary
}

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
    simulate_ohlcv(
        Ohlcv::close_only(close, open),
        signals,
        &SimOptions::new(initial_cash, bars_per_year, exec),
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
    simulate_ohlcv(
        Ohlcv::new(open, high, low, close),
        signals,
        &SimOptions {
            initial_cash,
            bars_per_year,
            exec,
            risk,
            sizing: &SizingConfig::AllIn,
        },
    )
}

/// 盘中回测主入口(带止损与仓位模式)。
///
/// 与 [`simulate_ohlc`] 使用**同一条**指标计算路径; 仓位模式由
/// [`SimOptions::sizing`] 指定。
///
/// # Errors
/// 执行费率非法、止损参数越界、仓位参数越界, 或配置了任一种盘中依赖
/// (止损/ATR 仓位)但 `high` / `low` 短于 `close` 时返回错误。
pub fn simulate_ohlcv(
    ohlcv: Ohlcv<'_>,
    signals: &[Signal],
    opts: &SimOptions<'_>,
) -> crate::error::Result<SummaryMetrics> {
    let n = validate_inputs(ohlcv, signals, opts)?;
    if n < 2 {
        return Ok(empty_summary(opts));
    }
    // collect_curves = false: 不物化曲线与逐笔 Trade; 曲线统计在线累计,
    // 逐笔 pnl 仍保留(O(交易笔数))用于高阶矩与分半 PF。
    let out = scan_trades(
        ohlcv.high,
        ohlcv.low,
        ohlcv.close,
        ohlcv.open,
        signals,
        &[],
        opts.initial_cash,
        opts.exec,
        opts.risk,
        opts.sizing,
        false,
    )?;
    debug_assert!(
        !out.collect_curves && out.detail.trades.is_empty(),
        "simulate 不得物化落盘产物"
    );
    let mut summary = compute_metrics(&out.metrics, opts.initial_cash, opts.bars_per_year)?;
    summary.apply_exec(opts.exec);
    Ok(summary)
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
    simulate_ohlcv_detailed(
        time,
        Ohlcv::new(open, high, low, close),
        signals,
        &SimOptions {
            initial_cash,
            bars_per_year,
            exec,
            risk,
            sizing: &SizingConfig::AllIn,
        },
    )
}

/// 详细回测主入口(带止损与仓位模式)。
///
/// 与 [`simulate_ohlcv`] 使用**同一条**指标计算路径, 仅额外物化逐笔交易与曲线。
///
/// # Errors
/// 同 [`simulate_ohlcv`]。
pub fn simulate_ohlcv_detailed(
    time: &[i64],
    ohlcv: Ohlcv<'_>,
    signals: &[Signal],
    opts: &SimOptions<'_>,
) -> crate::error::Result<SimResult> {
    let n = validate_inputs(ohlcv, signals, opts)?;
    if n < 2 {
        return Ok(SimResult {
            summary: empty_summary(opts),
            trades: Vec::new(),
            equity_curve: Vec::new(),
            fixed_curve: Vec::new(),
        });
    }
    // detailed 路径要落盘, 故额外物化逐笔 Trade 与两条曲线。
    // 注意指标仍只经 `out.metrics` 计算 —— 与 `simulate` 完全同源。
    let out = scan_trades(
        ohlcv.high,
        ohlcv.low,
        ohlcv.close,
        ohlcv.open,
        signals,
        time,
        opts.initial_cash,
        opts.exec,
        opts.risk,
        opts.sizing,
        true,
    )?;
    let mut summary = compute_metrics(&out.metrics, opts.initial_cash, opts.bars_per_year)?;
    summary.apply_exec(opts.exec);

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
