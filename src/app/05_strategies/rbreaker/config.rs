use serde::Deserialize;

use crate::config::F64Param;

/// R-Breaker 六价位突破+反转。六价公式固定(枢轴 P=(H+C+L)/3 的 myquant 经典式),
/// 唯一自由参数为止损百分比(原 50 点为 AG 专用, 改百分比通用化)。
/// 日内 UTC 日界强制平仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RBreakerConfig {
    pub stop_pct: F64Param,
}

impl RBreakerConfig {
    pub fn values(&self) -> Vec<f64> {
        self.stop_pct.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = RBreakerConfig {
            stop_pct: F64Param::Range(RangeParam {
                start: 0.5,
                end: 2.0,
                step: 1.5,
            }),
        };
        assert_eq!(c.values(), vec![0.5, 2.0]);
    }
}
