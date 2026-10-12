use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+振幅扩张过滤（入场验证#2，用户设计）：
/// 当根振幅与过去 n 根对比：`(high−low) > 1.5 × mean(high−low, n)`，
/// 且方向一致（多头 close>open），进场仍 BB20σ突破。
/// n 用户指定 {3,5,7,9,11,13,15}；mult=1.5 固定（单参数纪律）。
/// 与验证#1对照：方向要求相同，差异只在强度定义
/// （根内实体占比 vs 跨根振幅扩张）。离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkRexConfig {
    pub sigma: F64Param,
    pub rexn: F64Param,
}

impl BbrkRexConfig {
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let ss = self.sigma.values();
        let ns = self.rexn.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &n in &ns {
                out.push((s, n.round() as usize));
            }
        }
        out
    }
}

/// 扩张倍数固定值（1.5，经典扩张阈值；mult 敏感性留待后续）。
pub const REX_MULT: f64 = 1.5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkRexConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            rexn: F64Param::Range(RangeParam {
                start: 3.0,
                end: 5.0,
                step: 2.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.0, 3), (2.0, 5), (2.5, 3), (2.5, 5)]);
    }
}
