use crate::config::F64Param;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// Keltner(EMA20 ± k·ATR14)穿越，k 变。与 ATRBand 唯一变量是中线，逐格对照。
    pub struct KeltnerConfig;
    fields {
        /// 阈值倍数。
        mult: F64Param as f64, key "k", label "keltner.mult", range (0.1, 20.0);
    }
    expand values;
    code |_text| code::k_mult(mult);
    filter true;
    empty "keltner 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = KeltnerConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
