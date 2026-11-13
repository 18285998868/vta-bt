use vector_ta::indicators::moving_averages::dema::{DemaInput, DemaParams, dema};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **DEMA** 策略实现。
pub struct DemaCalc;

impl MaCalculator for DemaCalc {
    const NAME: &'static str = "DEMA";
    const KEY: &'static str = "dema";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = DemaInput::from_slice(
            close,
            DemaParams {
                period: Some(period),
            },
        );
        let ma = dema(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `DemaCalc` 输出的均线。
pub type DemaStrategy = SingleMaStrategy<DemaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::DemaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = DemaStrategy::new(RangeParam {
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
