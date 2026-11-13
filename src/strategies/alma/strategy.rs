use vector_ta::indicators::moving_averages::alma::{AlmaInput, AlmaParams, alma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **ALMA** 策略实现。
pub struct AlmaCalc;

impl MaCalculator for AlmaCalc {
    const NAME: &'static str = "ALMA";
    const KEY: &'static str = "alma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = AlmaInput::from_slice(
            close,
            AlmaParams {
                period: Some(period),
                offset: None,
                sigma: None,
            },
        );
        let ma = alma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `AlmaCalc` 输出的均线。
pub type AlmaStrategy = SingleMaStrategy<AlmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::AlmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = AlmaStrategy::new(RangeParam {
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
