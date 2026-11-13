use vector_ta::indicators::moving_averages::zlema::{ZlemaInput, ZlemaParams, zlema};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **ZLEMA** 策略实现。
pub struct ZlemaCalc;

impl MaCalculator for ZlemaCalc {
    const NAME: &'static str = "ZLEMA";
    const KEY: &'static str = "zlema";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = ZlemaInput::from_slice(
            close,
            ZlemaParams {
                period: Some(period),
            },
        );
        let ma = zlema(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `ZlemaCalc` 输出的均线。
pub type ZlemaStrategy = SingleMaStrategy<ZlemaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    use super::ZlemaStrategy;

    #[test]
    fn test_zlema_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = ZlemaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
