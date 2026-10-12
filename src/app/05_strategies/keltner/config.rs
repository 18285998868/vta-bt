use serde::Deserialize;

use crate::config::F64Param;

/// Keltner(EMA20 ± k·ATR14)穿越，k 变。与 ATRBand 唯一变量是中线，逐格对照。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeltnerConfig {
    pub mult: F64Param,
}

impl KeltnerConfig {
    pub fn values(&self) -> Vec<f64> {
        self.mult.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = KeltnerConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
