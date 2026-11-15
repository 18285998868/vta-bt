use crate::config::PeriodParam;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 唐奇安通道突破(研究口径, 只做多): 收盘突破前 N 日最高开多,
    /// 收盘跌破前 M 日最低平仓。与内置 `turtle` 的区别是**不做空**,
    /// 状态机只有 long/flat 两态, 与 `vbt-唐奇安通道突破.py` 的
    /// `entries = close > donchian_high` / `exits = close < donchian_low` 同义。
    pub struct DonchianLongConfig;
    fields {
        /// 入场通道周期。
        entry: PeriodParam as usize, key "entry", label "donchian_long.entry", validate 1;
        /// 离场通道周期。
        exit: PeriodParam as usize, key "exit", label "donchian_long.exit", validate 1;
    }
    expand pairs;
    code |_text| code::hi_lo(entry, exit);
    filter true;
    empty "donchian_long 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian_entry_exit() {
        let c = DonchianLongConfig {
            entry: PeriodParam::Range(RangeParam {
                start: 20,
                end: 55,
                step: 35,
            }),
            exit: PeriodParam::Range(RangeParam {
                start: 10,
                end: 20,
                step: 10,
            }),
        };
        assert_eq!(c.pairs(), vec![(20, 10), (20, 20), (55, 10), (55, 20)]);
    }

    #[test]
    fn param_text_is_stable() {
        let p = DonchianLongConfig::param_set(&(20, 10));
        assert_eq!(p.text(), "entry=20,exit=10");
        assert_eq!(p.json(), r#"{"entry":"20","exit":"10"}"#);
    }
}
