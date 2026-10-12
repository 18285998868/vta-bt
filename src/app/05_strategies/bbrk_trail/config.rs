use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR追踪离场(@2)：进场与 BBRK 完全相同（BB20σ突破），
/// 离场改用 Chandelier 式追踪（多头 close < 近N根最高high − k·ATR14 即走，
/// 空头镜像），不再碰中线就走。N=22/ATR14 固定，σ × k 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTrailConfig {
    pub sigma: F64Param,
    pub trailk: F64Param,
}

impl BbrkTrailConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let ks = self.trailk.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &k in &ks {
                out.push((s, k));
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
        let c = BbrkTrailConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            trailk: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 1.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 2.0), (2.0, 3.0), (2.5, 2.0), (2.5, 3.0)]
        );
    }
}
