use serde::Deserialize;

use crate::config::F64Param;

/// BBRK 布林趋势突破(WikiFX-EA口径): BB(20,σ)收盘突破上轨做多、
/// 跌破下轨做空，回到中线平仓。σ 变；周期固定20(布林经典值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBRKConfig {
    pub sigma: F64Param,
}

impl BBRKConfig {
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BBRKConfig {
            sigma: F64Param::Range(RangeParam { start: 1.5, end: 2.5, step: 0.5 }),
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
