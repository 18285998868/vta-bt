use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK中轨对照（基线验证#1）：BB周期参数化 + 中轨类型对照。
    /// len{5,10,15,20,25,30} × σ{2,2.5,3} × ma{sma,ema,hma,zlema}。
    /// 决策与 BBRK 逐位一致（flat+收盘突破上轨多/下轨空，中线离场），
    /// 只是 BB 的 period/matype 可变。sma/len20 应复刻 BBRK（ sanity 锚点）。
    pub struct BbrkBbConfig;
    fields {
        /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
        ma: Vec<String> as String, key "ma", label "bbrk_bb.ma",
            names crate::bbrk_bb::BB_MA_NAMES;
        /// 布林带周期。
        bblen: PeriodParam as usize, key "len", label "bbrk_bb.bblen", range (2, 2000);
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_bb.sigma", range (0.1, 10.0);
    }
    expand triples;
    code |_text| code::ma_bb(
        code::name_index(&crate::bbrk_bb::BB_MA_NAMES, &ma),
        bblen,
        Some(sigma),
    );
    filter true;
    empty "bbrk_bb 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn triples_cartesian() {
        let c = BbrkBbConfig {
            ma: vec!["sma".into()],
            bblen: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            sigma: F64Param::Single(2.5),
        };
        assert_eq!(
            c.triples(),
            vec![("sma".to_string(), 5, 2.5), ("sma".to_string(), 10, 2.5)]
        );
    }

    #[test]
    fn unknown_name_rejected() {
        let c = BbrkBbConfig {
            ma: vec!["nope".into()],
            bblen: PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
        };
        assert!(c.validate().is_err()); // 原 validate_names, 已并入 validate
    }
}
