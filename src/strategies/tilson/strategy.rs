use vector_ta::indicators::moving_averages::tilson::{TilsonInput, TilsonParams, tilson};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **Tilson** 策略实现。
pub struct TilsonCalc;

impl MaCalculator for TilsonCalc {
    const NAME: &'static str = "Tilson";
    const KEY: &'static str = "t3";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = TilsonInput::from_slice(
            close,
            TilsonParams {
                period: Some(period),
                volume_factor: None,
            },
        );
        let ma = tilson(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `TilsonCalc` 输出的均线。
pub type TilsonStrategy = SingleMaStrategy<TilsonCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::TilsonStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = TilsonStrategy::new(RangeParam {
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
