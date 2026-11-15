use crate::config::PeriodParam;
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 双均线比大小配置(官方 `docs/tutorials/ema_cross.py` 口径)。
    /// fast/slow 各为单值或范围, 实际组合只取 fast<slow(对角绞杀区天然排除)。
    pub struct EmaCrossConfig;
    fields {
        /// 快线周期。
        fast: PeriodParam as usize, key "fast", label "ema_cross.fast", validate 1;
        /// 慢线周期。
        slow: PeriodParam as usize, key "slow", label "ema_cross.slow", validate 1;
    }
    expand pairs;
    code |_text| code::hi_lo(fast, slow);
    filter (fast < slow);
    empty "ema_cross 无 fast<slow 有效组合";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    fn cfg(f0: usize, f1: usize, s0: usize, s1: usize) -> EmaCrossConfig {
        EmaCrossConfig {
            fast: PeriodParam::Range(RangeParam {
                start: f0,
                end: f1,
                step: 5,
            }),
            slow: PeriodParam::Range(RangeParam {
                start: s0,
                end: s1,
                step: 10,
            }),
        }
    }

    #[test]
    fn pairs_keep_only_fast_lt_slow() {
        let c = cfg(5, 50, 20, 200);
        let pairs = c.pairs();
        assert_eq!(pairs.len(), 174);
        assert!(pairs.iter().all(|(f, s)| f < s));
        assert!(pairs.contains(&(5, 20)) && pairs.contains(&(50, 200)));
    }

    #[test]
    fn pairs_empty_when_no_valid_combo() {
        let c = cfg(200, 200, 20, 20);
        assert!(c.pairs().is_empty());
    }

    #[test]
    fn single_values_form_one_pair() {
        let c = EmaCrossConfig {
            fast: PeriodParam::Single(50),
            slow: PeriodParam::Single(200),
        };
        assert_eq!(c.pairs(), vec![(50, 200)]);
    }
}
