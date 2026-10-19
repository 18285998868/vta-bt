use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};

/// **MCGINLEY** 策略实现。
pub struct McGinleyStrategy {
    periods: Vec<usize>,
}

impl McGinleyStrategy {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
    #[must_use]
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

fn mcgd_ma(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if n < 2 {
        return out;
    }
    let k = 0.6;
    let mut md = close[0];
    out[0] = md;
    for i in 1..n {
        if md > 1e-12 && period > 0 {
            let ratio = close[i] / md;
            let beta = k * period as f64 * ratio.powi(4);
            md += (close[i] - md) / beta.max(1.0);
        } else {
            md = close[i];
        }
        out[i] = md;
    }
    out
}

impl Strategy for McGinleyStrategy {
    fn name(&self) -> &'static str {
        "McGinley"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("mcginley_{}", self.periods[idx])
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
        let period = self.periods[param_idx];
        let ma = mcgd_ma(close, period);
        Ok(price_cross_signal(&ma, close))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::McGinleyStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = McGinleyStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
