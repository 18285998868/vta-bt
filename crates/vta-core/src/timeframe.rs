//! 周期与年化 bar 数。
//!
//! 这张表是回测引擎、分析层与 PBO 的**共同口径**。未知周期若静默按 1d
//! 年化, 夏普会差数十到上千倍且无提示, 故调用方必须先
//! [`crate::timeframe::is_known_timeframe`] 再取 [`crate::timeframe::bars_per_year`]。

/// 已知周期 → 年化 bar 数。
///
/// 按 24×365 的自然时间折算, 不考虑交易所节假日(加密市场连续交易)。
/// 1w 取 52 周。
pub const TIMEFRAME_BARS_PER_YEAR: &[(&str, f64)] = &[
    ("1m", 525_600.0),
    ("3m", 175_200.0),
    ("5m", 105_120.0),
    ("15m", 35_040.0),
    ("30m", 17_520.0),
    ("1h", 8_760.0),
    ("2h", 4_380.0),
    ("4h", 2_190.0),
    ("6h", 1_460.0),
    ("8h", 1_095.0),
    ("12h", 730.0),
    ("1d", 365.0),
    ("1w", 52.0),
];

/// 年化 bar 数。用于把逐 bar 收益换算成年化夏普/波动率。
///
/// 未列出的周期会落到 365, 这是**兼容旧行为**的兜底; 新调用方必须先检查
/// [`is_known_timeframe`], 否则会把 1m 当成 1d 年化。
#[must_use]
pub fn bars_per_year(tf: &str) -> f64 {
    TIMEFRAME_BARS_PER_YEAR
        .iter()
        .find(|(name, _)| *name == tf)
        .map_or(365.0, |(_, bpy)| *bpy)
}

/// [`bars_per_year`] 是否认识该周期。
#[must_use]
pub fn is_known_timeframe(tf: &str) -> bool {
    TIMEFRAME_BARS_PER_YEAR.iter().any(|(name, _)| *name == tf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timeframes_are_positive_and_consistent() {
        assert_eq!(bars_per_year("1h"), 8_760.0);
        assert_eq!(bars_per_year("1d"), 365.0);
        assert_eq!(bars_per_year("1w"), 52.0);
        assert!(is_known_timeframe("15m"));
        assert!(!is_known_timeframe("3s"));
        assert!(!is_known_timeframe(""));
        for (tf, bpy) in TIMEFRAME_BARS_PER_YEAR {
            assert!(is_known_timeframe(tf));
            assert!(*bpy > 0.0);
        }
    }

    #[test]
    fn bar_duration_is_inverse_of_bars_per_year() {
        let minutes = |tf: &str| bars_per_year("1m") / bars_per_year(tf);
        assert!((minutes("1h") - 60.0).abs() < 1e-9);
        assert!((minutes("1d") - 1_440.0).abs() < 1e-9);
        // 年化表按 52 周折算, 故 1w 是 525_600/52 分钟, 而非整 7×1440=10_080。
        assert!((minutes("1w") - 525_600.0 / 52.0).abs() < 1e-9);
    }
}
