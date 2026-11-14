use vector_ta::indicators::moving_averages::sinwma::{SinWmaInput, SinWmaParams, sinwma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **SinWMA** 策略实现。
pub struct SinWmaCalc;

impl MaCalculator for SinWmaCalc {
    const NAME: &'static str = "SinWMA";
    const KEY: &'static str = "sinwma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = SinWmaInput::from_slice(
            close,
            SinWmaParams {
                period: Some(period),
            },
        );
        let ma = sinwma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `SinWmaCalc` 输出的均线。
pub type SinWmaStrategy = SingleMaStrategy<SinWmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::SinWmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = SinWmaStrategy::new(RangeParam {
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
