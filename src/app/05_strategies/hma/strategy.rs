use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

pub struct HmaStrategy {
    periods: Vec<usize>,
}

impl HmaStrategy {
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for HmaStrategy {
    fn name(&self) -> &'static str {
        "HMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("hma_{}", self.periods[idx])
    }

    fn generate_signals(&self, close: &[f64], _volume: &[f64], param_idx: usize) -> Vec<Signal> {
        let period = self.periods[param_idx];
        let input = HmaInput::from_slice(
            close,
            HmaParams {
                period: Some(period),
            },
        );
        let ma = match hma(&input) {
            Ok(v) => v.values,
            Err(_) => return vec![Signal::Hold; close.len()],
        };
        price_cross_signal(&ma, close)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    use super::HmaStrategy;

    #[test]
    fn test_hma_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = HmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s.generate_signals(&close, &[], 0);
        assert!(sig.contains(&Signal::Buy));
    }
}
