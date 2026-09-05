use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};

pub struct VidyaStrategy {
    periods: Vec<usize>,
}

impl VidyaStrategy {
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

fn vidya_ma(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if n < period + 2 {
        return out;
    }
    let alpha_base = 2.0 / (period as f64 + 1.0);
    let mut v = close[0];
    let mut gains = 0.0;
    let mut losses = 0.0;
    for i in 1..=period {
        let diff = close[i] - close[i - 1];
        if diff > 0.0 {
            gains += diff;
        } else {
            losses -= diff;
        }
        v = alpha_base * close[i] + (1.0 - alpha_base) * v;
    }
    out[period] = v;
    let mut cmo_gains = gains;
    let mut cmo_losses = losses;
    for i in (period + 1)..n {
        let diff = close[i] - close[i - 1];
        let prev_diff = close[i - period] - close[i - period - 1];
        if diff > 0.0 {
            cmo_gains += diff - prev_diff.max(0.0);
            cmo_losses -= (-diff).max(0.0) - prev_diff.min(0.0).abs();
        } else {
            cmo_losses -= diff;
            cmo_gains -= prev_diff.max(0.0);
        }
        let denom = cmo_gains + cmo_losses;
        let cmo = if denom > 1e-12 {
            (cmo_gains - cmo_losses) / denom
        } else {
            0.0
        };
        let alpha = alpha_base * cmo.abs();
        v = alpha * close[i] + (1.0 - alpha) * v;
        out[i] = v;
    }
    out
}

impl Strategy for VidyaStrategy {
    fn name(&self) -> &'static str {
        "VIDYA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("vidya_{}", self.periods[idx])
    }

    fn generate_signals(&self, close: &[f64], _volume: &[f64], param_idx: usize) -> Vec<Signal> {
        let period = self.periods[param_idx];
        let ma = vidya_ma(close, period);
        price_cross_signal(&ma, close)
    }
}

#[cfg(test)]
mod tests {
    use super::VidyaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = VidyaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s.generate_signals(&close, &[], 0);
        assert!(sig.contains(&Signal::Buy));
    }
}
