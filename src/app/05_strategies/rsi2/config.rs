use serde::Deserialize;

use crate::config::RangeParam;

/// RSI(2) Connors 短线反转。buy/sell 笛卡尔积(经典 (10,90)/(5,95) 及交叉对，
/// 预注册在内)。离场：多头 RSI>50 / 空头 RSI<50(均值回归过半，Step2 锁定)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RSI2Config {
    pub buy: RangeParam<f64>,
    pub sell: RangeParam<f64>,
}

impl RSI2Config {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let bs = crate::strategies::expand_f64_range(self.buy.start, self.buy.end, self.buy.step);
        let ss =
            crate::strategies::expand_f64_range(self.sell.start, self.sell.end, self.sell.step);
        let mut out = Vec::new();
        for &b in &bs {
            for &s in &ss {
                out.push((b, s));
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
        let c = RSI2Config {
            buy: RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            },
            sell: RangeParam {
                start: 90.0,
                end: 95.0,
                step: 5.0,
            },
        };
        assert_eq!(
            c.pairs(),
            vec![(5.0, 90.0), (5.0, 95.0), (10.0, 90.0), (10.0, 95.0)]
        );
    }
}
