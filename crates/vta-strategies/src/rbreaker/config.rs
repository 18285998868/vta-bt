use crate::config::F64Param;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// R-Breaker 六价位突破+反转。六价公式固定(枢轴 P=(H+C+L)/3 的 myquant 经典式),
    /// 唯一自由参数为止损百分比(原 50 点为 AG 专用, 改百分比通用化)。
    /// 日内 UTC 日界强制平仓。
    pub struct RBreakerConfig;
    fields {
        /// 止损百分比。
        stop_pct: F64Param as f64, key "stop", label "rbreaker.stop_pct", range (0.01, 50.0);
    }
    expand values;
    code |_text| code::stop_pct(stop_pct);
    filter true;
    empty "rbreaker 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = RBreakerConfig {
            stop_pct: F64Param::Range(RangeParam {
                start: 0.5,
                end: 2.0,
                step: 1.5,
            }),
        };
        assert_eq!(c.values(), vec![0.5, 2.0]);
    }
}
