use crate::{Bars, ParamSet, Signal, Strategy, rolling_highest, rolling_lowest, shift_right_1};

use super::config::DonchianLongConfig;

/// 唐奇安通道突破(研究口径, 只做多)。
///
/// 与 `vbt-唐奇安通道突破.py` 的 `entries = close > donchian_high` /
/// `exits = close < donchian_low` 逐 bar 同义:
/// - 收盘价高于**前 N 根**(不含当根)最高价 → `Buy`;
/// - 收盘价低于**前 M 根**最低价 → `Close`;
/// - 出场优先于入场(研究状态机同一根先置 1 再置 0);
/// - 通道未预热(NaN) → `Hold`。
///
/// 研究口径是**水平触发**而不是"上穿": 起点重置为空仓后, 只要当下收盘
/// 已在通道外, 状态机就应给出对应目标; 用 `>` 的条件信号配合引擎的
/// 目标状态机, 天然得到 long/flat 两态, 不需要在策略内再维护一份状态。
pub struct DonchianLongStrategy {
    pairs: Vec<(usize, usize)>,
}

impl DonchianLongStrategy {
    /// 由 [`DonchianLongConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: DonchianLongConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测): 出场优先, NaN → `Hold`。
#[must_use]
pub fn donchian_long_signals(close: &[f64], entry_hi: &[f64], exit_lo: &[f64]) -> Vec<Signal> {
    let n = close.len().min(entry_hi.len()).min(exit_lo.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 0..n {
        if !close[i].is_finite() {
            continue;
        }
        if exit_lo[i].is_finite() && close[i] < exit_lo[i] {
            sig[i] = Signal::Close;
        } else if entry_hi[i].is_finite() && close[i] > entry_hi[i] {
            sig[i] = Signal::Buy;
        }
    }
    sig
}

fn signals_from_ohlc(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    entry: usize,
    exit: usize,
) -> Vec<Signal> {
    let entry_hi = shift_right_1(&rolling_highest(high, entry));
    let exit_lo = shift_right_1(&rolling_lowest(low, exit));
    donchian_long_signals(close, &entry_hi, &exit_lo)
}

impl Strategy for DonchianLongStrategy {
    fn name(&self) -> &'static str {
        "DONCHIAN_LONG"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_set(&self, idx: usize) -> ParamSet {
        DonchianLongConfig::param_set(&self.pairs[idx])
    }

    fn generate_signals_full(
        &self,
        bars: Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let (entry, exit) = self.pairs[param_idx];
        Ok(signals_from_ohlc(
            bars.high(),
            bars.low(),
            bars.close(),
            entry,
            exit,
        ))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_entry_and_exit_are_long_flat_only() {
        let close = [95.0, 101.0, 102.0, 89.0, 89.5, 88.0, 101.0];
        let hi = [100.0; 7];
        let lo = [90.0; 7];
        let sig = donchian_long_signals(&close, &hi, &lo);
        assert_eq!(sig[0], Signal::Hold);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
        assert_eq!(sig[4], Signal::Close);
        assert_eq!(sig[6], Signal::Buy);
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn exit_wins_when_both_channels_are_breached() {
        // 病态输入(exit_lo > entry_hi): 出场优先, 与 build_state 同序。
        let close = [95.0];
        let hi = [90.0];
        let lo = [100.0];
        assert_eq!(donchian_long_signals(&close, &hi, &lo)[0], Signal::Close);
    }

    #[test]
    fn nan_channels_hold() {
        let close = [101.0, 102.0];
        let nan = [f64::NAN, f64::NAN];
        let sig = donchian_long_signals(&close, &nan, &nan);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn channels_are_shifted_one_bar() {
        // 前 3 根逐步抬高; 第 4 根收盘 105 突破前 3 根最高 103。
        let high = [100.0, 101.0, 103.0, 105.0];
        let low = [90.0, 90.0, 90.0, 90.0];
        let close = [99.0, 100.0, 102.0, 105.0];
        let sig = signals_from_ohlc(&high, &low, &close, 3, 3);
        assert_eq!(sig[3], Signal::Buy);
    }
}
