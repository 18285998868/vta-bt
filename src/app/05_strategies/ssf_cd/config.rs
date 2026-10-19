use serde::Deserialize;

use crate::config::PeriodParam;
use crate::strategies::PoleCount;

/// `ssf` 的 K 线方向过滤变体。参数与 `ssf` 完全一致, 仅多一个固定的方向约束。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SsfCdConfig {
    /// 指标周期(bar 数)。
    pub period: PeriodParam,
    /// 极数。2 = Ehlers 论文原版 (带预平均), 3 = *Cybernetic Analysis* ch.13。默认 2。
    #[serde(default)]
    /// 策略参数 `poles`。
    pub poles: PoleCount,
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poles_defaults_to_two_when_omitted() {
        let c: SsfCdConfig = toml::from_str("period = 20").unwrap();
        assert_eq!(c.poles, PoleCount::Two);
    }

    #[test]
    fn poles_accepts_two_and_three() {
        for (n, want) in [(2, PoleCount::Two), (3, PoleCount::Three)] {
            let src = format!("period = 20\npoles = {n}");
            let c: SsfCdConfig = toml::from_str(&src).unwrap();
            assert_eq!(c.poles, want);
        }
    }

    #[test]
    fn poles_rejects_other_values() {
        for n in [0, 1, 4, -1] {
            let src = format!("period = 20\npoles = {n}");
            assert!(
                toml::from_str::<SsfCdConfig>(&src).is_err(),
                "poles = {n} 应被拒绝"
            );
        }
    }
}
