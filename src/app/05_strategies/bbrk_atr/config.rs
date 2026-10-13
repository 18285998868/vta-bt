use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR扩张门（入场验证#4）：进场仍 BB突破 + 方向一致，
/// 叠加 `ATR5/ATR20 > thr`（Wilder口径，长度5/20固定）。
/// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × len × thr 笛卡尔积。
/// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
/// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAtrConfig {
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    pub atrthr: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkAtrConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let ts = self.atrthr.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                for &t in &ts {
                    out.push((s, l.round() as usize, t));
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
        let c: BbrkAtrConfig = toml::from_str("sigma = 2.5\natrthr = 1.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 1.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAtrConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 0.8), (2.5, 20, 1.0)]);
    }
}
