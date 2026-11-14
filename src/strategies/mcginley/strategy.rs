use crate::strategies::single_ma::{MaCalculator, SingleMaStrategy};

/// **McGinley** 策略实现。
pub struct McGinleyCalc;

impl MaCalculator for McGinleyCalc {
    const NAME: &'static str = "McGinley";
    const KEY: &'static str = "mcginley";

    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>> {
        Ok(mcgd_ma(close, period))
    }
}

/// 单 MA 策略: 价格穿越 `McGinleyCalc` 输出的均线。
pub type McGinleyStrategy = SingleMaStrategy<McGinleyCalc>;

#[must_use]
fn mcgd_ma(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if n < 2 {
        return out;
    }
    let k = 0.6;
    let mut md = close[0];
    out[0] = md;
    for i in 1..n {
        if md > 1e-12 && period > 0 {
            let ratio = close[i] / md;
            let beta = k * period as f64 * ratio.powi(4);
            md += (close[i] - md) / beta.max(1.0);
        } else {
            md = close[i];
        }
        out[i] = md;
    }
    out
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::McGinleyStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = McGinleyStrategy::new(RangeParam {
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
