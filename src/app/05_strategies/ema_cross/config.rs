use serde::Deserialize;

use crate::config::PeriodParam;

/// 双均线比大小配置(官方 `docs/tutorials/ema_cross.py` 口径)。
/// fast/slow 各为单值或范围, 实际组合只取 fast<slow(对角绞杀区天然排除)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmaCrossConfig {
    pub fast: PeriodParam,
    pub slow: PeriodParam,
}

impl EmaCrossConfig {
    /// 有效参数对(至少 1 对, 否则配置无意义)。
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let fasts = self.fast.to_range();
        let slows = self.slow.to_range();
        let f: Vec<usize> = (fasts.start..=fasts.end).step_by(fasts.step).collect();
        let s: Vec<usize> = (slows.start..=slows.end).step_by(slows.step).collect();
        let mut out = Vec::new();
        for &ff in &f {
            for &ss in &s {
                if ff < ss {
                    out.push((ff, ss));
                }
            }
        }
        out
    }
}

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
