use serde::Deserialize;

use crate::config::F64Param;

/// 正统布林形态对照（Bollinger 2001 第二章 W 底 / M 顶 + %b 背离，
/// 2026-10-11）：BB(20, sma, σ) 经典口径；形态回看窗 W 预注册固定为 10
/// 根（见 `ORTH_W`，不调参，防 P2），σ{2.0,2.5,3.0} = 3组+BBRK对照
/// （IS单窗108单元预注册）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkOrthConfig {
    pub sigma: F64Param,
}

impl BbrkOrthConfig {
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
        let c = BbrkOrthConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
