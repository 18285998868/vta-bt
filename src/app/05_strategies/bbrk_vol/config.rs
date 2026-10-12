use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+量能门（入场验证#3，用户设计）：
/// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
/// 进场仍 BB20σ突破 + 方向一致（与STR/REX同口径）。
/// 离场保持中线。σ × N × m 三笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkVolConfig {
    pub sigma: F64Param,
    pub voln: F64Param,
    pub volmult: F64Param,
}

impl BbrkVolConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, f64)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &n in &self.voln.values() {
                for &m in &self.volmult.values() {
                    out.push((s, n.round() as usize, m));
                }
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
        let c = BbrkVolConfig {
            sigma: F64Param::Single(2.5),
            voln: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
            volmult: F64Param::Single(1.5),
        };
        assert_eq!(c.pairs(), vec![(2.5, 5, 1.5), (2.5, 10, 1.5)]);
    }
}
