use vector_ta::indicators::moving_averages::smma::{SmmaInput, SmmaParams, smma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **Smma** 策略实现。
pub struct SmmaCalc;

impl MaCalculator for SmmaCalc {
    const NAME: &'static str = "Smma";
    const KEY: &'static str = "smma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = SmmaInput::from_slice(
            close,
            SmmaParams {
                period: Some(period),
            },
        );
        let ma = smma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `SmmaCalc` 输出的均线。
pub type SmmaStrategy = SingleMaStrategy<SmmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::SmmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = SmmaStrategy::new(RangeParam {
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
