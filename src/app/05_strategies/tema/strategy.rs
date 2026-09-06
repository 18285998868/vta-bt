use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::tema::{TemaInput, TemaParams, tema};

pub struct TemaStrategy {
    periods: Vec<usize>,
}

impl TemaStrategy {
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for TemaStrategy {
    fn name(&self) -> &'static str {
        "TEMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("tema_{}", self.periods[idx])
    }

    fn generate_signals(
        &self,
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        let input = TemaInput::from_slice(
            close,
            TemaParams {
                period: Some(period),
            },
        );
        let ma = match tema(&input) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(price_cross_signal(&ma, close))
    }
}

#[cfg(test)]
mod tests {
    use super::TemaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = TemaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        assert_eq!(s.param_count(), 1);
        let sig = s.generate_signals(&close, &[], 0).unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
