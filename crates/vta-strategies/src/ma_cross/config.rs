use crate::config::PeriodParam;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 双均线交叉(研究口径, 只做多): fast 上穿 slow 开多, 下穿平仓。
    /// 与研究脚本 `vbt-双均线交叉.py` 的 `cross_signals` 同义 —— 出场是
    /// **平仓**而不是反手, 故本策略不产出 `Sell`。
    pub struct MaCrossConfig;
    fields {
        /// 均线类型名列表(取值见 [`MA_CROSS_NAMES`](crate::ma_cross::MA_CROSS_NAMES))。
        ma: Vec<String> as String, key "ma", label "ma_cross.ma",
            names crate::ma_cross::MA_CROSS_NAMES;
        /// 快线周期。
        fast: PeriodParam as usize, key "fast", label "ma_cross.fast", validate 1;
        /// 慢线周期。
        slow: PeriodParam as usize, key "slow", label "ma_cross.slow", validate 1;
    }
    expand triples;
    code |_text| code::packed(&[
        (code::name_index(&crate::ma_cross::MA_CROSS_NAMES, &ma) as u64, 4),
        (fast as u64, 14),
        (slow as u64, 14),
    ]);
    filter (fast < slow);
    empty "ma_cross 无 fast<slow 有效组合";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn triples_keep_only_fast_lt_slow() {
        let c = MaCrossConfig {
            ma: vec!["sma".into()],
            fast: PeriodParam::Range(RangeParam {
                start: 20,
                end: 60,
                step: 40,
            }),
            slow: PeriodParam::Range(RangeParam {
                start: 60,
                end: 200,
                step: 140,
            }),
        };
        assert_eq!(
            c.triples(),
            vec![
                ("sma".to_string(), 20, 60),
                ("sma".to_string(), 20, 200),
                ("sma".to_string(), 60, 200),
            ]
        );
    }

    #[test]
    fn param_text_and_code_are_stable() {
        let p = MaCrossConfig::param_set(&("ema".to_string(), 50, 200));
        assert_eq!(p.text(), "ma=ema,fast=50,slow=200");
        assert_eq!(p.json(), r#"{"ma":"ema","fast":"50","slow":"200"}"#);
        assert_eq!(
            p.code(),
            crate::params::code::packed(&[(1, 4), (50, 14), (200, 14)])
        );
    }

    #[test]
    fn unknown_ma_name_rejected() {
        let c = MaCrossConfig {
            ma: vec!["wma".into()],
            fast: PeriodParam::Single(20),
            slow: PeriodParam::Single(60),
        };
        assert!(c.validate().is_err());
    }
}
