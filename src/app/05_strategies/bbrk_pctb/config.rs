use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+%b离场(@2)：进场与 BBRK 相同（BB20σ突破），
/// 离场改 %b 阈值（多头 %b < exitb 即走，空头 %b > 1−exitb 即走；
/// %b=(close−lower)/(upper−lower)，上轨=1/下轨=0/中线=0.5）。
/// exitb=0.5 恒等于中线离场（内置健全性检查，与 BBRK 逐位一致）。
/// σ × exitb 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkPctbConfig {
    pub sigma: F64Param,
    pub exitb: F64Param,
}

impl BbrkPctbConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let bs = self.exitb.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                out.push((s, b));
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
        let c = BbrkPctbConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            exitb: F64Param::Range(RangeParam {
                start: 0.2,
                end: 0.5,
                step: 0.3,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 0.2), (2.0, 0.5), (2.5, 0.2), (2.5, 0.5)]
        );
    }
}
