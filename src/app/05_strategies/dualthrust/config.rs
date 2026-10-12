use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// Dual Thrust 开盘区间突破。N=回看天数, k=k1=k2 对称系数(经典默认 0.2)。
/// Range=max(HH−LC, HC−LL), 上轨=当日开+k·R, 下轨=当日开−k·R。
/// 持仓过夜(与 myquant 示例一致, Step2 已锁定)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualThrustConfig {
    pub n: PeriodParam,
    pub k: F64Param,
}

impl DualThrustConfig {
    pub fn pairs(&self) -> Vec<(usize, f64)> {
        let r = self.n.to_range();
        let ns: Vec<usize> = (r.start..=r.end).step_by(r.step).collect();
        let ks = self.k.values();
        let mut out = Vec::new();
        for &a in &ns {
            for &b in &ks {
                out.push((a, b));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};

    #[test]
    fn pairs_cartesian() {
        let c = DualThrustConfig {
            n: PeriodParam::Range(RangeParam {
                start: 3,
                end: 5,
                step: 2,
            }),
            k: F64Param::Range(RangeParam {
                start: 0.1,
                end: 0.2,
                step: 0.1,
            }),
        };
        assert_eq!(c.pairs(), vec![(3, 0.1), (3, 0.2), (5, 0.1), (5, 0.2)]);
    }
}
