use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+SMA regime门（EMA基线版：BB中轨ema，门线仍SMA(L)，其余与 bbrk_sma 逐位一致）：
/// close > SMA(L) 只允许多头（空头突破拦掉），close < SMA(L) 只允许空头；
/// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
/// 离场保持中线。SMA(L) 手算（与 BB 中线 SMA20 同口径），NaN/预热不足 → 拦。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkSmaEmaConfig {
    pub sigma: F64Param,
    pub smareg: F64Param,
}

impl BbrkSmaEmaConfig {
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let ss = self.sigma.values();
        let ls = self.smareg.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                out.push((s, l.round() as usize));
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
        let c = BbrkSmaEmaConfig {
            sigma: F64Param::Single(2.5),
            smareg: F64Param::Range(RangeParam {
                start: 50.0,
                end: 100.0,
                step: 50.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 50), (2.5, 100)]);
    }
}
