use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// Dual Thrust 开盘区间突破。N=回看天数, k=k1=k2 对称系数(经典默认 0.2)。
    /// Range=max(HH−LC, HC−LL), 上轨=当日开+k·R, 下轨=当日开−k·R。
    /// 持仓过夜(与 myquant 示例一致, Step2 已锁定)。
    pub struct DualThrustConfig;
    fields {
        /// 计数/统计窗口(bar 数)。
        n: PeriodParam as usize, key "n", label "dualthrust.n", validate 1;
        /// 阈值倍数(常为 ATR / 通道倍数)。
        k: F64Param as f64, key "k", label "dualthrust.k", range (0.01, 5.0);
    }
    expand pairs;
    code |_text| code::n_second(n, k);
    filter true;
    empty "dualthrust 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

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
