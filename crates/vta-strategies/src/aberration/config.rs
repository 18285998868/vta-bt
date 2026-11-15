use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// Aberration 波动率通道突破(思想级复现，非逐行：上轨=HH(N)+M·ATR(N)，
    /// 下轨=LL(N)−M·ATR(N)，穿越翻仓，无日内平仓，长持仓)。
    pub struct AberrationConfig;
    fields {
        /// 计数/统计窗口(bar 数)。
        n: PeriodParam as usize, key "n", label "aberration.n", validate 1;
        /// 阈值倍数。
        mult: F64Param as f64, key "m", label "aberration.mult", range (0.1, 20.0);
    }
    expand pairs;
    code |_text| code::n_second(n, mult);
    filter true;
    empty "aberration 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = AberrationConfig {
            n: PeriodParam::Range(RangeParam {
                start: 20,
                end: 50,
                step: 30,
            }),
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(20, 1.5), (20, 2.5), (50, 1.5), (50, 2.5)]);
    }
}
