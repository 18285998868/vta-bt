use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+TTM Squeeze正统版（用户设计2）：Keltner(EMA20 ± k·ATR14) 上下轨
/// 落在布林带上下轨内部即 squeeze_on；过去 n 根（含当根）存在 squeeze_on
/// 且当根 BB 突破 → 进；离场保持中线。
/// σ × k × n 三笛卡尔（用户指定 k{1,1.5,2,2.5}；n 取 {5,10} 紧/松两档）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTtmConfig {
    pub sigma: F64Param,
    pub kc: F64Param,
    pub ttmn: F64Param,
}

impl BbrkTtmConfig {
    pub fn pairs(&self) -> Vec<(f64, f64, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &k in &self.kc.values() {
                for &n in &self.ttmn.values() {
                    out.push((s, k, n.round() as usize));
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
        let c = BbrkTtmConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            kc: F64Param::Single(1.5),
            ttmn: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 1.5, 5), (2.0, 1.5, 10), (2.5, 1.5, 5), (2.5, 1.5, 10)]
        );
    }
}
