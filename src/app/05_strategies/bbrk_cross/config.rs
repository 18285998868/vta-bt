use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+前根确认（前根位置对照实验，2026-10-11）：决策与 `bbrk_signals`
/// 逐位一致，仅多一条件——进场要求前一根收盘在轨内（含触线）。
/// 首根/前根缺失或 NaN → 不开仓（P7）。
/// σ{2.0,2.5,3.0} = 3组+BBRK对照（IS单窗108单元预注册）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkCrossConfig {
    pub sigma: F64Param,
}

impl BbrkCrossConfig {
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
        let c = BbrkCrossConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
