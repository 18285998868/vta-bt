use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+归一化带宽阈值（用户设计1）：进场仍 BB20σ突破，只允许
/// `Bandwidth < mult × ATR14/mid` 时进（Bandwidth=(upper−lower)/mid；
/// ATR 同除 mid 归一化，跨币种×周期可比）。离场保持中线。
/// 量级推导（随机游走近似）：20根std ≈ 2.24×ATR，σ=2 时比值中枢 ≈9，
/// 收窄时掉到一半以下，故 mult 取 {3,6,9} 覆盖紧/中/松；保留率会验证
/// 量级假设（全≈100% 或全≈0% 即假设错）。σ × mult 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkBwConfig {
    pub sigma: F64Param,
    pub bwmult: F64Param,
}

impl BbrkBwConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let ms = self.bwmult.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &m in &ms {
                out.push((s, m));
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
        let c = BbrkBwConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bwmult: F64Param::Range(RangeParam {
                start: 3.0,
                end: 6.0,
                step: 3.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 3.0), (2.0, 6.0), (2.5, 3.0), (2.5, 6.0)]
        );
    }
}
