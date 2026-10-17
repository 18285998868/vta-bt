use crate::strategies::{Signal, Strategy};
use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};

use super::config::EmaCrossConfig;

/// 双均线比大小(官方 `docs/tutorials/ema_cross.py` 口径):
/// `fast>=slow` 持多、`fast<slow` 持空; 首个双有效 bar 按当下状态进场(官方 flat 即进场),
/// 之后只在状态翻转处给信号。与引擎 `signals_to_positions`(Buy→1/Sell→-1/Hold→保持)
/// 组合后, 持仓与官方逐 bar 状态机逐位一致(含 `>=` 偏多与未初始化 Hold)。
pub struct EmaCrossStrategy {
    pairs: Vec<(usize, usize)>,
}

impl EmaCrossStrategy {
    pub fn new(config: EmaCrossConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

fn ema_values(close: &[f64], period: usize) -> Option<Vec<f64>> {
    let input = EmaInput::from_slice(
        close,
        EmaParams {
            period: Some(period),
        },
    );
    ema(&input).ok().map(|v| v.values)
}

impl Strategy for EmaCrossStrategy {
    fn name(&self) -> &'static str {
        "EMA_CROSS"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (f, s) = self.pairs[idx];
        format!("fast={f},slow={s}")
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        _high: &[f64],
        _low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (fast_p, slow_p) = self.pairs[param_idx];
        let (Some(fast), Some(slow)) = (ema_values(close, fast_p), ema_values(close, slow_p))
        else {
            return Ok(vec![Signal::Hold; close.len()]);
        };
        let n = close.len().min(fast.len()).min(slow.len());
        let mut sig = vec![Signal::Hold; n];
        // 首个双有效 bar 按状态进场, 之后只在翻转处给信号。
        let mut prev: Option<bool> = None;
        for i in 0..n {
            if fast[i].is_nan() || slow[i].is_nan() {
                continue;
            }
            let long = fast[i] >= slow[i];
            match prev {
                None => sig[i] = if long { Signal::Buy } else { Signal::Sell },
                Some(p) => {
                    if long && !p {
                        sig[i] = Signal::Buy;
                    } else if !long && p {
                        sig[i] = Signal::Sell;
                    }
                }
            }
            prev = Some(long);
        }
        Ok(sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};
    use crate::strategies::Strategy;

    fn cfg_one(f: usize, s: usize) -> EmaCrossConfig {
        EmaCrossConfig {
            fast: PeriodParam::Single(f),
            slow: PeriodParam::Single(s),
        }
    }

    #[test]
    fn test_name_and_params() {
        let s = EmaCrossStrategy::new(cfg_one(10, 20));
        assert_eq!(s.name(), "EMA_CROSS");
        assert_eq!(s.param_count(), 1);
        assert_eq!(s.param_str(0), "fast=10,slow=20");
    }

    #[test]
    fn test_monotonic_up_enters_long_once() {
        // 单调上涨: 快线恒在慢线上方 → 首个有效 bar 给 Buy, 之后全 Hold。
        let close: Vec<f64> = (0..300).map(|i| 100.0 + i as f64).collect();
        let s = EmaCrossStrategy::new(cfg_one(10, 20));
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 300);
        let buys: Vec<usize> = sig
            .iter()
            .enumerate()
            .filter(|(_, x)| **x == Signal::Buy)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(buys.len(), 1);
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_flip_gives_sell_then_buy() {
        // 先涨后跌再涨: 至少一次 Sell 翻转(状态机可达性, 非精确计数)。
        let mut close: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.5).collect();
        close.extend((0..200).map(|i| 200.0 - i as f64 * 0.5));
        close.extend((0..200).map(|i| 100.0 + i as f64 * 0.5));
        let s = EmaCrossStrategy::new(cfg_one(5, 20));
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
        assert!(sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_range_config_counts_valid_pairs() {
        let c = EmaCrossConfig {
            fast: PeriodParam::Range(RangeParam {
                start: 5,
                end: 50,
                step: 5,
            }),
            slow: PeriodParam::Range(RangeParam {
                start: 20,
                end: 200,
                step: 10,
            }),
        };
        assert_eq!(EmaCrossStrategy::new(c).param_count(), 174);
    }
}
