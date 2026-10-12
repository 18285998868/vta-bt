use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR扩张门（入场验证#4）：进场仍 BB20σ突破 + 方向一致，
/// 叠加 `ATR5/ATR20 > thr`（Wilder口径，长度5/20固定）。
/// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × thr 笛卡尔积。
/// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAtrConfig {
    pub sigma: F64Param,
    pub atrthr: F64Param,
}

impl BbrkAtrConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let ts = self.atrthr.values();
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
        let c = BbrkAtrConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 0.8), (2.0, 1.0), (2.5, 0.8), (2.5, 1.0)]
        );
    }
}
