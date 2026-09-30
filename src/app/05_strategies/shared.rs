#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Hold,
    Buy,
    Sell,
    Close,
}

pub fn price_cross_signal(indicator: &[f64], close: &[f64]) -> Vec<Signal> {
    let n = close.len().min(indicator.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 1..n {
        if indicator[i].is_nan() || indicator[i - 1].is_nan() {
            continue;
        }
        if close[i - 1] < indicator[i - 1] && close[i] >= indicator[i] {
            sig[i] = Signal::Buy;
        } else if close[i - 1] > indicator[i - 1] && close[i] <= indicator[i] {
            sig[i] = Signal::Sell;
        }
    }
    sig
}

/// 年化 bar 数。用于把逐 bar 收益换算成年化夏普/波动率。
///
/// **未列出的周期会静默落到 365, 造成年化结果成倍错误**, 故调用方必须在配置校验阶段
/// 用 [`is_known_timeframe`] 拦下未知周期。
pub fn bars_per_year(tf: &str) -> f64 {
    match tf {
        "1m" => 525_600.0,
        "3m" => 175_200.0,
        "5m" => 105_120.0,
        "15m" => 35_040.0,
        "30m" => 17_520.0,
        "1h" => 8_760.0,
        "2h" => 4_380.0,
        "4h" => 2_190.0,
        "6h" => 1_460.0,
        "8h" => 1_095.0,
        "12h" => 730.0,
        "1d" => 365.0,
        "1w" => 52.0,
        _ => 365.0,
    }
}

/// `bars_per_year` 是否认识该周期。配置校验用, 避免未知周期静默按 1d 年化。
pub fn is_known_timeframe(tf: &str) -> bool {
    matches!(
        tf,
        "1m" | "3m" | "5m" | "15m" | "30m" | "1h" | "2h" | "4h" | "6h" | "8h" | "12h" | "1d" | "1w"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_cross_buy_then_sell() {
        let close = vec![10.0, 12.0, 15.0, 14.0, 18.0];
        let indicator = vec![11.0, 13.0, 13.0, 15.0, 16.0];
        let sig = price_cross_signal(&indicator, &close);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Sell);
    }

    #[test]
    fn test_price_cross_all_hold_when_nan() {
        let close = vec![10.0, 11.0, 12.0];
        let indicator = vec![f64::NAN, f64::NAN, f64::NAN];
        let sig = price_cross_signal(&indicator, &close);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_bars_per_year() {
        assert_eq!(bars_per_year("1h"), 8760.0);
        assert_eq!(bars_per_year("1d"), 365.0);
        assert_eq!(bars_per_year("unknown"), 365.0);
    }

    #[test]
    fn test_bars_per_year_covers_all_known_timeframes() {
        // 数据目录里实际存在的 12 个周期, 一个都不能漏
        for tf in [
            "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d",
        ] {
            assert!(is_known_timeframe(tf), "{tf} 未被识别");
            // 年化 bar 数必须与周期长度成反比: 相邻周期比值 = 时长比
            assert!(bars_per_year(tf) > 52.0, "{tf} 年化 bar 数异常偏小");
        }
        assert!(is_known_timeframe("1w"));
        assert!(!is_known_timeframe("3s"));
        assert!(!is_known_timeframe(""));
    }

    #[test]
    fn test_bars_per_year_inverse_to_bar_duration() {
        // 1m 一年 = 60 * 24 * 365 = 525600; 每个周期按其分钟数等比缩放
        let minutes = |tf: &str| bars_per_year("1m") / bars_per_year(tf);
        assert_eq!(minutes("1m"), 1.0);
        assert_eq!(minutes("3m"), 3.0);
        assert_eq!(minutes("5m"), 5.0);
        assert_eq!(minutes("1h"), 60.0);
        assert_eq!(minutes("4h"), 240.0);
        assert_eq!(minutes("1d"), 1440.0);
    }
}
