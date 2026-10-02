use serde::Deserialize;

use crate::config::{PeriodParam, RangeParam};

/// Aberration 波动率通道突破(思想级复现，非逐行：上轨=HH(N)+M·ATR(N)，
/// 下轨=LL(N)−M·ATR(N)，穿越翻仓，无日内平仓，长持仓)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AberrationConfig {
    pub n: PeriodParam,
    pub mult: RangeParam<f64>,
}

impl AberrationConfig {
    pub fn pairs(&self) -> Vec<(usize, f64)> {
        let r = self.n.to_range();
        let ns: Vec<usize> = (r.start..=r.end).step_by(r.step).collect();
        let ms =
            crate::strategies::expand_f64_range(self.mult.start, self.mult.end, self.mult.step);
        let mut out = Vec::new();
        for &a in &ns {
            for &b in &ms {
                out.push((a, b));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_cartesian() {
        let c = AberrationConfig {
            n: PeriodParam::Range(RangeParam {
                start: 20,
                end: 50,
                step: 30,
            }),
            mult: RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            },
        };
        assert_eq!(c.pairs(), vec![(20, 1.5), (20, 2.5), (50, 1.5), (50, 2.5)]);
    }
}
