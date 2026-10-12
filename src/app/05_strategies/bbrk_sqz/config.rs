use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+收窄过滤(@1正统形态)：进场仍是 BB20σ突破，但只允许在
/// Bandwidth 处分位时进（当前带宽 ≤ 过去N根带宽的下四分位，q=0.25 固定；
/// 文献口径 Bandwidth=(upper−lower)/mid，TTM Squeeze 的无Keltner版）。
/// 离场保持中线（隔离进场过滤效应）。σ × N 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkSqzConfig {
    pub sigma: F64Param,
    pub sqzn: F64Param,
}

impl BbrkSqzConfig {
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let ss = self.sigma.values();
        let ns = self.sqzn.values();
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
        let c = BbrkSqzConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            sqzn: F64Param::Range(RangeParam {
                start: 20.0,
                end: 50.0,
                step: 30.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.0, 20), (2.0, 50), (2.5, 20), (2.5, 50)]);
    }
}
