use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 均线 + ATR 通道策略配置。
    pub struct MaAtrConfig;
    fields {
        /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
        ma: Vec<String> as String, key "ma", label "ma_atr.ma",
            names crate::mabb_short::MABB_MA_NAMES;
        /// 均线周期。
        len: PeriodParam as usize, key "len", label "ma_atr.len", range (1, 2000);
        /// 阈值倍数。
        mult: F64Param as f64, key "mult", label "ma_atr.mult", range (0.1, 20.0);
        /// ATR 周期。
        atrlen: PeriodParam as usize, key "atrlen", label "ma_atr.atrlen", range (2, 500);
    }
    expand quints;
    code |_text| code::packed(&[
        (code::name_index(&crate::mabb_short::MABB_MA_NAMES, &ma) as u64, 6),
        (len as u64, 11),
        (code::scaled(mult, 1000.0), 15),
        (atrlen as u64, 9),
    ]);
    filter true;
    empty "ma_atr 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quints_cartesian() {
        let c = MaAtrConfig {
            ma: vec!["sma".into()],
            len: PeriodParam::Single(20),
            mult: F64Param::Single(2.0),
            atrlen: PeriodParam::Single(14),
        };
        assert_eq!(c.quints(), vec![("sma".to_string(), 20, 2.0, 14)]);
    }
}
