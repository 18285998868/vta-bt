use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};

use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::KeltnerConfig;

/// Keltner(EMA20 ± k·ATR14)穿越, k 变。与 ATRBand 唯一变量是中线, 逐格对照。
/// 上穿上轨 Buy, 下穿下轨 Sell。NaN→Hold。
pub struct KeltnerStrategy {
    mults: Vec<f64>,
}

impl KeltnerStrategy {
    pub fn new(config: KeltnerConfig) -> Self {
        Self {
            mults: config.values(),
        }
    }
}

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    mult: f64,
) -> crate::error::Result<Vec<Signal>> {
    let basis = match ema(&EmaInput::from_slice(close, EmaParams { period: Some(20) })) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(14) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let n = close.len().min(basis.len()).min(atr_vals.len());
    let mut up = vec![f64::NAN; n];
    let mut dn = vec![f64::NAN; n];
    for i in 0..n {
        if basis[i].is_finite() && atr_vals[i].is_finite() {
            up[i] = basis[i] + mult * atr_vals[i];
            dn[i] = basis[i] - mult * atr_vals[i];
        }
    }
    Ok(price_cross_signal(&up, close)
        .iter()
        .zip(price_cross_signal(&dn, close).iter())
        .map(|(u, d)| match (u, d) {
            (Signal::Buy, _) => Signal::Buy,
            (_, Signal::Sell) => Signal::Sell,
            _ => Signal::Hold,
        })
        .collect())
}

impl Strategy for KeltnerStrategy {
    fn name(&self) -> &'static str {
        "KELTNER"
    }

    fn param_count(&self) -> usize {
        self.mults.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("k={}", self.mults[idx])
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        compute(close, close, close, self.mults[param_idx])
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
        compute(high, low, close, self.mults[param_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_jump_up_buys_no_sells() {
        // 微振荡基线(ATR>0 使带与价格分离)后跳涨: 跳涨根上穿上轨给 Buy。
        let mut close: Vec<f64> = (0..60).map(|i| 100.0 + (i % 2) as f64).collect();
        close.extend((0..60).map(|i| 130.0 + (i % 2) as f64));
        let sig = compute(&close, &close, &close, 2.0).unwrap();
        assert!(sig.contains(&Signal::Buy));
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_name_and_params() {
        let s = KeltnerStrategy { mults: vec![1.5] };
        assert_eq!(s.name(), "KELTNER");
        assert_eq!(s.param_str(0), "k=1.5");
    }
}
