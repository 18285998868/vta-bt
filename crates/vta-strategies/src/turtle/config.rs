use crate::config::PeriodParam;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 海龟 Donchian 突破。entry/exit 独立笛卡尔(经典 20/10 与 55/20 皆在其中)。
    /// 策略本身只产出信号; ATR 风险仓位由 `[[strategy]].sizing` 的 `type = "atr"`
    /// 配置(见 [`vta_core::engine::SizingConfig`]), 不再需要固定名义近似。
    pub struct TurtleConfig;
    fields {
        /// 入场通道周期。
        entry: PeriodParam as usize, key "entry", label "turtle.entry", validate 1;
        /// 离场通道周期。
        exit: PeriodParam as usize, key "exit", label "turtle.exit", validate 1;
    }
    expand pairs;
    code |_text| code::hi_lo(entry, exit);
    filter true;
    empty "turtle 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian_entry_exit() {
        let c = TurtleConfig {
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
}
