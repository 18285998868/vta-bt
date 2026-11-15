use crate::config::F64Param;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// VWAP拉伸回归：session VWAP(日界重置, 典型价加权)偏离超 th·ATR后首次收回。
    /// th 变；多空对称同一阈值。
    pub struct VwapConfig;
    fields {
        /// 方向判定阈值。
        thresh: F64Param as f64, key "thresh", label "vwap.thresh", range (0.5, 10.0);
    }
    expand values;
    code |_text| code::packed(&[(code::scaled(thresh, 1000.0), 16)]);
    filter true;
    empty "vwap 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = VwapConfig {
            thresh: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
