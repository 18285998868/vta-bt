use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};

use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **EMA** 策略实现。
pub struct EmaCalc;

impl MaCalculator for EmaCalc {
    const NAME: &'static str = "EMA";
    const KEY: &'static str = "ema";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        let input = EmaInput::from_slice(
            close,
            EmaParams {
                period: Some(period),
            },
        );
        let ma = ema(&input).map_err(crate::strategies::param_skipped)?;
        Ok(ma.values)
    }
}

/// 单 MA 策略: 价格穿越 `EmaCalc` 输出的均线。
pub type EmaStrategy = SingleMaStrategy<EmaCalc>;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    use super::EmaStrategy;

    #[test]
    fn test_ema_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = EmaStrategy::new(RangeParam {
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
