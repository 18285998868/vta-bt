use vector_ta::indicators::moving_averages::tema::{TemaInput, TemaParams, tema};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **TEMA** 策略实现。
pub struct TemaCalc;

impl MaCalculator for TemaCalc {
    const NAME: &'static str = "TEMA";
    const KEY: &'static str = "tema";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = TemaInput::from_slice(
            close,
            TemaParams {
                period: Some(period),
            },
        );
        let ma = tema(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `TemaCalc` 输出的均线。
pub type TemaStrategy = SingleMaStrategy<TemaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::TemaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = TemaStrategy::new(RangeParam {
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
