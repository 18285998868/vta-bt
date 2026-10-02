use serde::Deserialize;

use crate::config::RangeParam;

/// Keltner(EMA20 ± k·ATR14)穿越，k 变。与 ATRBand 唯一变量是中线，逐格对照。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeltnerConfig {
    pub mult: RangeParam<f64>,
}

impl KeltnerConfig {
    pub fn values(&self) -> Vec<f64> {
        crate::strategies::expand_f64_range(self.mult.start, self.mult.end, self.mult.step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_expand() {
        let c = KeltnerConfig {
            mult: RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            },
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
