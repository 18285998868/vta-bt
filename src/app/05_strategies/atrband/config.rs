use serde::Deserialize;

use crate::config::RangeParam;

/// ATR 通道突破(中线 SMA20 + ATR14 固定，变 k)。穿越翻仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ATRBandConfig {
    pub mult: RangeParam<f64>,
}

impl ATRBandConfig {
    pub fn values(&self) -> Vec<f64> {
        crate::strategies::expand_f64_range(self.mult.start, self.mult.end, self.mult.step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_expand() {
        let c = ATRBandConfig {
            mult: RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            },
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
