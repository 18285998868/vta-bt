use vector_ta::indicators::moving_averages::edma::{EdmaInput, EdmaParams, edma};

use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::EdmaConfig;

/// EDMA 策略：价格穿越延迟版 EMA。
/// `edma[i] = ema[i - offset]`，offset=0 时严格退化为 EMA。
pub struct EdmaStrategy {
    pairs: Vec<(usize, usize)>,
}

impl EdmaStrategy {
    pub fn new(config: EdmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for EdmaStrategy {
    fn name(&self) -> &'static str {
        "EDMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (p, o) = self.pairs[idx];
        format!("edma_p{}_o{}", p, o)
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
        let (period, offset) = self.pairs[param_idx];
        let ma = match edma(&EdmaInput::from_slice(
            close,
            EdmaParams {
                period: Some(period),
                offset: Some(offset),
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
        let cfg = EdmaConfig {
            period: PeriodParam::Single(14),
            offset: Some(PeriodParam::Single(2)),
        };
        let s = EdmaStrategy::new(cfg);
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
