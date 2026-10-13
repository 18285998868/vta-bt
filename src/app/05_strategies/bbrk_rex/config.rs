use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+振幅扩张过滤（入场验证#2，用户设计）：
/// 当根振幅与过去 n 根对比：`(high−low) > k × mean(high−low, n)`，
/// 且方向一致（多头 close>open），进场仍 BB20σ突破。
/// n 用户指定 {3,5,7,9,11,13,15}；k{1.5,2,2.5,3}（2026-10-12起参数化，
/// 原mult=1.5固定；旧"sigma=S,rexn=N"参数串封存，新串多",rexmult=M"后缀）。
/// BB周期bblen（2026-10-12起参数化，默认20即经典；旧串视为len20）。
/// 与验证#1对照：方向要求相同，差异只在强度定义
/// （根内实体占比 vs 跨根振幅扩张）。离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkRexConfig {
    pub sigma: F64Param,
    /// BB周期（2026-10-12起参数化；缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    pub rexn: F64Param,
    pub rexmult: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkRexConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let ns = self.rexn.values();
        let ms = self.rexmult.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                for &n in &ns {
                    for &m in &ms {
                        out.push((s, l.round() as usize, n.round() as usize, m));
                    }
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
    fn missing_bblen_defaults_to_20() {
        let c: BbrkRexConfig = toml::from_str("sigma = 2.5\nrexn = 7.0\nrexmult = 2.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 7, 2.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkRexConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            rexn: F64Param::Range(RangeParam {
                start: 3.0,
                end: 5.0,
                step: 2.0,
            }),
            rexmult: F64Param::Single(2.0),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 3, 2.0), (2.5, 20, 5, 2.0)]);
    }
}
