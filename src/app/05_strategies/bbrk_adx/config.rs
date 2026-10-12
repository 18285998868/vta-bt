use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ADX 震荡过滤(BBRK 突破 + ADX14 门限): 只在 `adx >= thr` 时允许开仓，
/// 回到中线无条件平仓。σ × thr 笛卡尔积；ADX 周期固定 14(经典值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAdxConfig {
    pub sigma: F64Param,
    pub adx: F64Param,
}

impl BbrkAdxConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let ts = self.adx.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &t in &ts {
                out.push((s, t));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAdxConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            adx: F64Param::Range(RangeParam {
                start: 20.0,
                end: 25.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 20.0), (2.0, 25.0), (2.5, 20.0), (2.5, 25.0)]
        );
    }
}
