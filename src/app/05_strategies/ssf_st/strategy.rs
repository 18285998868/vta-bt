use crate::strategies::{Signal, Strategy, price_cross_signal};

use super::config::SsfStConfig;

pub struct SsfStStrategy {
    periods: Vec<usize>,
}

impl SsfStStrategy {
    pub fn new(config: SsfStConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

fn ssf_filter(close: &[f64], period: usize) -> Vec<f64> {
    use std::f64::consts::PI;
    let x = PI * 2.0_f64.sqrt() / period as f64;
    let a = (-x).exp();
    let a2 = a * a;
    let b = 2.0 * a * x.cos();
    let c1 = 1.0 + a2 - b;
    let n = close.len();
    let mut values = vec![0.0; n];
    if n > 0 {
        values[0] = close[0];
    }
    if n > 1 {
        values[1] = close[1];
    }
    for i in 2..n {
        values[i] = c1 * close[i] + b * values[i - 1] - a2 * values[i - 2];
    }
    values
}

impl Strategy for SsfStStrategy {
    fn name(&self) -> &'static str {
        "SSF_ST"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("period={},variant=ssf_st", self.periods[idx])
    }

    fn generate_signals(&self, close: &[f64], _volume: &[f64], param_idx: usize) -> Vec<Signal> {
        let period = self.periods[param_idx];
        let values = ssf_filter(close, period);
        price_cross_signal(&values, close)
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
    fn test_ssf_st_generates_signals() {
        let s = SsfStStrategy::new(SsfStConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
        });
        let sig = s.generate_signals(&prices(200), &[], 0);
        assert_eq!(sig.len(), 200);
    }
}
