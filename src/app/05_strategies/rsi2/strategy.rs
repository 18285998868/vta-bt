use vector_ta::indicators::rsi::{RsiInput, RsiParams, rsi};

use crate::strategies::{Signal, Strategy};

use super::config::RSI2Config;

/// RSI(2) Connors 短线反转: flat + rsi<buy → Buy; flat + rsi>sell → Sell;
/// long + rsi>50 → Close; short + rsi<50 → Close(均值回归过半离场)。
/// vector-ta RSI 为 0–100 经典口径。NaN→Hold。
pub struct RSI2Strategy {
    pairs: Vec<(f64, f64)>,
}

impl RSI2Strategy {
    /// 由 [`RSI2Config`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: RSI2Config) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn rsi2_signals(close: &[f64], rsi_vals: &[f64], buy_thr: f64, sell_thr: f64) -> Vec<Signal> {
    let n = close.len().min(rsi_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if !close[i].is_finite() || !rsi_vals[i].is_finite() {
            continue;
        }
        if pos == 0 {
            if rsi_vals[i] < buy_thr {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if rsi_vals[i] > sell_thr {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if rsi_vals[i] > 50.0 {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if rsi_vals[i] < 50.0 {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

impl Strategy for RSI2Strategy {
    fn name(&self) -> &'static str {
        "RSI2"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (b, s) = self.pairs[idx];
        format!("buy={b},sell={s}")
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
        let (b, s) = self.pairs[param_idx];
        let rsi_vals = match rsi(&RsiInput::from_slice(close, RsiParams { period: Some(2) })) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(rsi2_signals(close, &rsi_vals, b, s))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_oversold_buys_mid_closes() {
        // rsi: 5 Buy → 60 Close。
        let close = vec![100.0, 99.0, 100.0, 101.0];
        let rsi_vals = vec![50.0, 5.0, 40.0, 60.0];
        let sig = rsi2_signals(&close, &rsi_vals, 10.0, 90.0);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_overbought_sells() {
        let close = vec![100.0, 101.0, 100.0, 99.0];
        let rsi_vals = vec![50.0, 95.0, 60.0, 40.0];
        let sig = rsi2_signals(&close, &rsi_vals, 10.0, 90.0);
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = RSI2Strategy {
            pairs: vec![(10.0, 90.0)],
        };
        assert_eq!(s.name(), "RSI2");
        assert_eq!(s.param_str(0), "buy=10,sell=90");
    }
}
