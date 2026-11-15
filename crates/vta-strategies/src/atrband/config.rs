use crate::config::F64Param;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// ATR 通道突破(中线 SMA20 + ATR14 固定，变 k)。穿越翻仓。
    pub struct ATRBandConfig;
    fields {
        /// 阈值倍数。
        mult: F64Param as f64, key "k", label "atrband.mult", range (0.1, 20.0);
    }
    expand values;
    code |_text| code::k_mult(mult);
    filter true;
    empty "atrband 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = ATRBandConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
