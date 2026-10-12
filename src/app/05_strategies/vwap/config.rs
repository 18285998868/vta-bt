use serde::Deserialize;

use crate::config::F64Param;

/// VWAP拉伸回归：session VWAP(日界重置, 典型价加权)偏离超 th·ATR后首次收回。
/// th 变；多空对称同一阈值。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VwapConfig {
    pub thresh: F64Param,
}

impl VwapConfig {
    pub fn values(&self) -> Vec<f64> {
        self.thresh.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = VwapConfig {
            thresh: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
