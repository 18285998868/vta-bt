use vector_ta::indicators::moving_averages::fwma::{FwmaInput, FwmaParams, fwma};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **FWMA** 策略实现。
pub struct FwmaCalc;

impl MaCalculator for FwmaCalc {
    const NAME: &'static str = "FWMA";
    const KEY: &'static str = "fwma";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = FwmaInput::from_slice(
            close,
            FwmaParams {
                period: Some(period),
            },
        );
        let ma = fwma(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `FwmaCalc` 输出的均线。
pub type FwmaStrategy = SingleMaStrategy<FwmaCalc>;

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
