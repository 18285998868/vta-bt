use vector_ta::indicators::moving_averages::swma::{SwmaInput, SwmaParams, swma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **SWMA** 策略实现。
pub struct SwmaCalc;

impl MaCalculator for SwmaCalc {
    const NAME: &'static str = "SWMA";
    const KEY: &'static str = "swma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = SwmaInput::from_slice(
            close,
            SwmaParams {
                period: Some(period),
            },
        );
        let ma = swma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `SwmaCalc` 输出的均线。
pub type SwmaStrategy = SingleMaStrategy<SwmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::SwmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = SwmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(
                crate::strategies::Bars::new(&[], &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
