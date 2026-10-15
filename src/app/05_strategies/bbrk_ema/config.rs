use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+EMA regime门：SMA门同形，regime线换 EMA（vector-ta 现成指标）。
/// close > EMA(L) 只允许多头，close < EMA(L) 只允许空头；
/// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
/// BB周期bblen（缺省20＝旧串语义）。
/// 离场保持中线。EMA(L) NaN/预热不足 → 拦。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkEmaConfig {
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    pub emareg: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkEmaConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ls = self.emareg.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                for &l in &ls {
                    out.push((s, b.round() as usize, l.round() as usize));
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
        let c: BbrkEmaConfig = toml::from_str("sigma = 2.5\nemareg = 100.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 100)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkEmaConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            emareg: F64Param::Range(RangeParam {
                start: 50.0,
                end: 100.0,
                step: 50.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
