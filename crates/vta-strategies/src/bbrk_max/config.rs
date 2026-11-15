use crate::config::PeriodParam;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+通用均线 regime门（入场验证#7）：12条均线谁来当 regime 线，
    /// 平滑是否影响过滤效果，一轮定案。
    /// close > MA(L) 只多，close < MA(L) 只空；L{100,200}；σ固定2.5
    /// （SMA/HMA版已证σ只平移不改变排序，固定中间档省算力）。
    /// 离场保持中线。
    pub struct BbrkMaxConfig;
    fields {
        /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
        ma: Vec<String> as String, key "ma", label "bbrk_max.ma",
            names crate::bbrk_max::MA_NAMES;
        /// regime 均线周期。
        malen: PeriodParam as usize, key "malen", label "bbrk_max.malen", range (2, 2000);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_max.bblen",
            default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::packed(&[
        (code::name_index(&crate::bbrk_max::MA_NAMES, &ma) as u64, 4),
        (malen as u64, 12),
        (bblen as u64, 16),
    ]);
    filter true;
    empty "bbrk_max 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkMaxConfig {
            ma: vec!["sma".into(), "hma".into()],
            malen: PeriodParam::Range(RangeParam {
                start: 100,
                end: 200,
                step: 100,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                ("sma".to_string(), 100, 20),
                ("sma".to_string(), 200, 20),
                ("hma".to_string(), 100, 20),
                ("hma".to_string(), 200, 20)
            ]
        );
    }

    #[test]
    fn unknown_name_rejected() {
        let c = BbrkMaxConfig {
            ma: vec!["nope".into()],
            malen: PeriodParam::Single(100),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert!(c.validate().is_err()); // 原 validate_names, 已并入 validate
    }
}
