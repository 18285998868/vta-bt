use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::strategies::{Signal, Strategy, rolling_highest, rolling_lowest, shift_right_1};

use super::config::{ClenowConfig, ClenowParams, TrendMode};

/// Clenow 混合趋势: 双均线 regime + 唐奇安突破。
///
/// 状态机保证引擎重放一致: 空仓看 regime + 突破入场; 多头跌破离场下轨平仓,
/// 空头突破离场上轨平仓。ATR 追踪止损与风险仓位由引擎配置, 不在策略内部。
pub struct ClenowStrategy {
    combos: Vec<ClenowParams>,
}

impl ClenowStrategy {
    /// 由 `ClenowConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: ClenowConfig) -> Self {
        Self {
            combos: config.combos(),
        }
    }
}

#[inline]
fn cross_up(close: &[f64], level: &[f64], i: usize) -> bool {
    let (a, b, c, d) = (close[i - 1], close[i], level[i - 1], level[i]);
    [a, b, c, d].iter().all(|v| v.is_finite()) && a <= c && b > d
}

#[inline]
fn cross_down(close: &[f64], level: &[f64], i: usize) -> bool {
    let (a, b, c, d) = (close[i - 1], close[i], level[i - 1], level[i]);
    [a, b, c, d].iter().all(|v| v.is_finite()) && a >= c && b < d
}

/// 纯决策函数(可手算单测): 均线 regime + 入场/离场通道。
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn clenow_signals(
    close: &[f64],
    fast_ma: &[f64],
    slow_ma: &[f64],
    entry_hi: &[f64],
    entry_lo: &[f64],
    exit_hi: &[f64],
    exit_lo: &[f64],
    allow_short: bool,
) -> Vec<Signal> {
    let n = [
        close.len(),
        fast_ma.len(),
        slow_ma.len(),
        entry_hi.len(),
        entry_lo.len(),
        exit_hi.len(),
        exit_lo.len(),
    ]
    .into_iter()
    .min()
    .unwrap_or(0);
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 1..n {
        if !(close[i - 1].is_finite()
            && close[i].is_finite()
            && fast_ma[i].is_finite()
            && slow_ma[i].is_finite())
        {
            continue;
        }
        if pos == 0 {
            if fast_ma[i] > slow_ma[i] && cross_up(close, entry_hi, i) {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if allow_short && fast_ma[i] < slow_ma[i] && cross_down(close, entry_lo, i) {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if cross_down(close, exit_lo, i) {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if cross_up(close, exit_hi, i) {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

fn sma_series(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
    let input = SmaInput::from_slice(
        close,
        SmaParams {
            period: Some(period),
        },
    );
    match sma(&input) {
        Ok(v) => Ok(v.values),
        Err(e) => Err(crate::strategies::param_skipped(e)),
    }
}

fn signals_from_ohlc(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    p: &ClenowParams,
) -> crate::error::Result<Vec<Signal>> {
    let fast = sma_series(close, p.fast)?;
    let slow = sma_series(close, p.slow)?;
    // 通道右移: 用前 N 根极值, 含当根的通道永远无法被穿越。
    let hh_e = shift_right_1(&rolling_highest(high, p.entry));
    let ll_e = shift_right_1(&rolling_lowest(low, p.entry));
    let hh_x = shift_right_1(&rolling_highest(high, p.exit));
    let ll_x = shift_right_1(&rolling_lowest(low, p.exit));
    Ok(clenow_signals(
        close,
        &fast,
        &slow,
        &hh_e,
        &ll_e,
        &hh_x,
        &ll_x,
        p.mode == TrendMode::LongShort,
    ))
}

impl Strategy for ClenowStrategy {
    fn name(&self) -> &'static str {
        "CLENOW"
    }

    fn param_count(&self) -> usize {
        self.combos.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let p = self.combos[idx];
        format!(
            "entry={},exit={},fast={},slow={},mode={}",
            p.entry,
            p.exit,
            p.fast,
            p.slow,
            p.mode.label()
        )
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        signals_from_ohlc(close, high, low, &self.combos[param_idx])
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PeriodParam;

    fn cfg() -> ClenowConfig {
        ClenowConfig {
            fast: PeriodParam::Single(50),
            slow: PeriodParam::Single(100),
            entry: PeriodParam::Single(20),
            exit: PeriodParam::Single(10),
            mode: crate::strategies::clenow::ModeList::Many(vec![
                TrendMode::LongShort,
                TrendMode::LongOnly,
            ]),
        }
    }

    #[test]
    fn regime_gates_long_entry() {
        let close = vec![100.0, 90.0, 101.0, 102.0];
        let hi = vec![100.0; 4];
        let lo = vec![0.0; 4];
        let exit_hi = vec![f64::INFINITY; 4];
        let exit_lo = vec![0.0; 4];
        let up = vec![1.0; 4];
        let down = vec![-1.0; 4];
        // 快线低于慢线: 突破也不做多。
        let sig = clenow_signals(&close, &down, &up, &hi, &lo, &exit_hi, &exit_lo, false);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Buy).count(), 0);
        // 快线高于慢线: 突破入多一次。
        let sig = clenow_signals(&close, &up, &down, &hi, &lo, &exit_hi, &exit_lo, false);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Buy).count(), 1);
    }

    #[test]
    fn long_only_ignores_short_entry() {
        let close = vec![100.0, 110.0, 90.0, 80.0, 70.0];
        let hi = vec![f64::INFINITY; 5];
        let lo = vec![95.0; 5];
        let exit_hi = vec![f64::INFINITY; 5];
        let exit_lo = vec![0.0; 5];
        let fast = vec![-1.0; 5];
        let slow = vec![1.0; 5];
        let sig = clenow_signals(&close, &fast, &slow, &hi, &lo, &exit_hi, &exit_lo, false);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Sell).count(), 0);
        let sig = clenow_signals(&close, &fast, &slow, &hi, &lo, &exit_hi, &exit_lo, true);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Sell).count(), 1);
    }

    #[test]
    fn exit_on_reverse_channel() {
        let close = vec![100.0, 101.0, 102.0, 90.0];
        let hi = vec![100.0; 4];
        let lo = vec![0.0; 4];
        let exit_hi = vec![f64::INFINITY; 4];
        let exit_lo = vec![0.0, 0.0, 0.0, 95.0];
        let up = vec![1.0; 4];
        let down = vec![-1.0; 4];
        let sig = clenow_signals(&close, &up, &down, &hi, &lo, &exit_hi, &exit_lo, false);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn param_grid_and_labels() {
        let s = ClenowStrategy::new(cfg());
        assert_eq!(s.param_count(), 2);
        assert_eq!(
            s.param_str(0),
            "entry=20,exit=10,fast=50,slow=100,mode=long_short"
        );
        assert_eq!(
            s.param_str(1),
            "entry=20,exit=10,fast=50,slow=100,mode=long_only"
        );
    }
}
