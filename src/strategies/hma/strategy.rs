use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **HMA** 策略实现。
pub struct HmaCalc;

impl MaCalculator for HmaCalc {
    const NAME: &'static str = "HMA";
    const KEY: &'static str = "hma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = HmaInput::from_slice(
            close,
            HmaParams {
                period: Some(period),
            },
        );
        let ma = hma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `HmaCalc` 输出的均线。
pub type HmaStrategy = SingleMaStrategy<HmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    use super::HmaStrategy;

    #[test]
    fn test_hma_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = HmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s
            .generate_signals_full(
                crate::strategies::Bars::new(&[], &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
