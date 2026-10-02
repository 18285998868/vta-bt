use serde::Deserialize;

use crate::config::RangeParam;

/// BB 均值回归(官方 `BBMeanReversion` 口径): BB(20,σ)+RSI(14)，
/// 碰下沿且 RSI<0.30 做多，碰上沿且 RSI>0.70 做空，回到中线平仓。
/// σ 变；RSI 阈值固定(官方值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBMRConfig {
    pub sigma: RangeParam<f64>,
}

impl BBMRConfig {
    pub fn values(&self) -> Vec<f64> {
        crate::strategies::expand_f64_range(self.sigma.start, self.sigma.end, self.sigma.step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_expand() {
        let c = BBMRConfig {
            sigma: RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            },
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
