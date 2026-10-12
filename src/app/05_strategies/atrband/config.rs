use serde::Deserialize;

use crate::config::F64Param;

/// ATR 通道突破(中线 SMA20 + ATR14 固定，变 k)。穿越翻仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ATRBandConfig {
    pub mult: F64Param,
}

impl ATRBandConfig {
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
        let c = ATRBandConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
