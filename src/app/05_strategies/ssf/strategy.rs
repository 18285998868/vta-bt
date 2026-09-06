use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::SsfConfig;

pub struct SsfStrategy {
    periods: Vec<usize>,
}

impl SsfStrategy {
    pub fn new(config: SsfConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for SsfStrategy {
    fn name(&self) -> &'static str {
        "SSF"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("period={},variant=ssf", self.periods[idx])
    }

    fn generate_signals(
        &self,
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        use vector_ta::indicators::moving_averages::supersmoother::{
            SuperSmootherInput, SuperSmootherParams, supersmoother,
        };

        let values = match supersmoother(&SuperSmootherInput::from_slice(
            close,
            SuperSmootherParams {
                period: Some(period),
            },
        )) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(price_cross_signal(&values, close))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};

    fn prices(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect()
    }

    #[test]
    fn test_ssf_standard() {
        let s = SsfStrategy::new(SsfConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
        });
        assert!(s.param_count() > 0);
        let sig = s.generate_signals(&prices(200), &[], 0).unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }
}
