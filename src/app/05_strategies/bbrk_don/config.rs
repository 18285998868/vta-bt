use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+Donchian极值离场(@2)：进场与 BBRK 相同（BB20σ突破），
/// 离场改过去 N 根极值（多头 close 跌破近N根最低low即走，空头镜像，
/// 均不含当根，Donchian 经典口径）。中线不再离场。
/// σ × N 笛卡尔积；N 取整使用。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkDonConfig {
    pub sigma: F64Param,
    pub exitn: F64Param,
}

impl BbrkDonConfig {
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let ss = self.sigma.values();
        let ns = self.exitn.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &n in &ns {
                out.push((s, n.round() as usize));
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
        let c = BbrkDonConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            exitn: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 1.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.0, 2), (2.0, 3), (2.5, 2), (2.5, 3)]);
    }
}
