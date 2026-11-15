use crate::config::F64Param;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// RSI(2) Connors 短线反转。buy/sell 笛卡尔积(经典 (10,90)/(5,95) 及交叉对，
    /// 预注册在内)。离场：多头 RSI>50 / 空头 RSI<50(均值回归过半，Step2 锁定)。
    pub struct RSI2Config;
    fields {
        /// 超卖阈值(RSI 量纲)。
        buy: F64Param as f64, key "buy", label "rsi2.buy", range (0.0, 100.0);
        /// 超买阈值(RSI 量纲)。
        sell: F64Param as f64, key "sell", label "rsi2.sell", range (0.0, 100.0);
    }
    expand pairs;
    code |_text| code::buy_sell(buy, sell);
    filter true;
    empty "rsi2 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = RSI2Config {
            buy: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
            sell: F64Param::Range(RangeParam {
                start: 90.0,
                end: 95.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(5.0, 90.0), (5.0, 95.0), (10.0, 90.0), (10.0, 95.0)]
        );
    }
}
