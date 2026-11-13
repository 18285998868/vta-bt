use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::fwma::{FwmaInput, FwmaParams, fwma};

/// **FWMA** 策略实现。
pub struct FwmaStrategy {
    periods: Vec<usize>,
}

impl FwmaStrategy {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
    #[must_use]
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for FwmaStrategy {
    fn name(&self) -> &'static str {
        "FWMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("fwma_{}", self.periods[idx])
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
        let input = FwmaInput::from_slice(
            close,
            FwmaParams {
                period: Some(period),
            },
        );
        let ma = match fwma(&input) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        Ok(price_cross_signal(&ma, close))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::FwmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = FwmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
