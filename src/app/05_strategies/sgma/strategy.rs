use vector_ta::indicators::moving_averages::sgf::{SgfInput, SgfParams, sgf};

use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::SgmaConfig;

/// SGMA 策略：价格穿越 Savitzky-Golay endpoint smoother。
pub struct SgmaStrategy {
    pairs: Vec<(usize, usize)>,
}

impl SgmaStrategy {
    pub fn new(config: SgmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for SgmaStrategy {
    fn name(&self) -> &'static str {
        "SGMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (p, o) = self.pairs[idx];
        format!("sgma_p{}_m{}", p, o)
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
        let (period, poly_order) = self.pairs[param_idx];
        let ma = match sgf(&SgfInput::from_slice(
            close,
            SgfParams {
                period: Some(period),
                poly_order: Some(poly_order),
            },
        )) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(price_cross_signal(&ma, close))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PeriodParam;

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let cfg = SgmaConfig {
            period: PeriodParam::Single(21),
            poly_order: None,
        };
        let s = SgmaStrategy::new(cfg);
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
