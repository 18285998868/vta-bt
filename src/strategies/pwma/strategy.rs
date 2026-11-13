use vector_ta::indicators::moving_averages::pwma::{PwmaInput, PwmaParams, pwma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **PWMA** 策略实现。
pub struct PwmaCalc;

impl MaCalculator for PwmaCalc {
    const NAME: &'static str = "PWMA";
    const KEY: &'static str = "pwma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = PwmaInput::from_slice(
            close,
            PwmaParams {
                period: Some(period),
            },
        );
        let ma = pwma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `PwmaCalc` 输出的均线。
pub type PwmaStrategy = SingleMaStrategy<PwmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::PwmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = PwmaStrategy::new(RangeParam {
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
