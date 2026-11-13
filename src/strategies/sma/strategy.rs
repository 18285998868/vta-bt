use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **SMA** 策略实现。
pub struct SmaCalc;

impl MaCalculator for SmaCalc {
    const NAME: &'static str = "SMA";
    const KEY: &'static str = "sma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = SmaInput::from_slice(
            close,
            SmaParams {
                period: Some(period),
            },
        );
        let ma = sma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `SmaCalc` 输出的均线。
pub type SmaStrategy = SingleMaStrategy<SmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    use super::SmaStrategy;

    #[test]
    fn test_sma_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = SmaStrategy::new(RangeParam {
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
