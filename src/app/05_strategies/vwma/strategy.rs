use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::vwma::{VwmaInput, VwmaParams, vwma};

pub struct VwmaStrategy {
    periods: Vec<usize>,
}

impl VwmaStrategy {
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for VwmaStrategy {
    fn name(&self) -> &'static str {
        "VWMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("vwma_{}", self.periods[idx])
    }

    fn generate_signals(&self, close: &[f64], volume: &[f64], param_idx: usize) -> Vec<Signal> {
        let period = self.periods[param_idx];
        let vol = if volume.len() == close.len() {
            volume
        } else {
            close
        };
        let input = VwmaInput::from_slice(
            close,
            vol,
            VwmaParams {
                period: Some(period),
            },
        );
        let ma = match vwma(&input) {
            Ok(v) => v.values,
            Err(_) => return vec![Signal::Hold; close.len()],
        };
        price_cross_signal(&ma, close)
    }
}

#[cfg(test)]
mod tests {
    use super::VwmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = VwmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s.generate_signals(&close, &[], 0);
        assert!(sig.iter().any(|&s| s == Signal::Buy));
    }
}
