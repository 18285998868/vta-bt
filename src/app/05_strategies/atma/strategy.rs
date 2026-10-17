use vector_ta::indicators::moving_averages::atma::{AtmaInput, AtmaParams, atma};

use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::AtmaConfig;

/// ATMA 策略：价格穿越非对称三角均线。
pub struct AtmaStrategy {
    pairs: Vec<(usize, usize)>,
}

impl AtmaStrategy {
    pub fn new(config: AtmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for AtmaStrategy {
    fn name(&self) -> &'static str {
        "ATMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (u, d) = self.pairs[idx];
        format!("atma_u{}_d{}", u, d)
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
        let (up, down) = self.pairs[param_idx];
        let ma = match atma(&AtmaInput::from_slice(
            close,
            AtmaParams {
                up_period: Some(up),
                down_period: Some(down),
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
        let cfg = AtmaConfig {
            up: PeriodParam::Single(10),
            down: PeriodParam::Single(20),
        };
        let s = AtmaStrategy::new(cfg);
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
