//! Shared helpers used by `app/`.

use std::path::Path;

const PAIR_ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
const TIMEFRAME_ALLOWED: &str = "0123456789abcdefghijklmnopqrstuvwxyz_";

fn is_allowed(s: &str, allowed: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| allowed.contains(b as char))
}

/// 交易对白名单校验（大写字母/数字/`_`，防目录穿越）。
pub fn valid_pair(s: &str) -> bool {
    is_allowed(s, PAIR_ALLOWED)
}

/// 周期白名单校验（小写字母/数字/`_`，防目录穿越）。
pub fn valid_timeframe(s: &str) -> bool {
    is_allowed(s, TIMEFRAME_ALLOWED)
}

/// 目录下是否存在 kline zip 文件。
pub fn has_kline_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("zip"))
        })
        .unwrap_or(false)
}

/// 周期排序权重（未知周期沉底）。
pub fn timeframe_order(tf: &str) -> u32 {
    match tf {
        "1s" => 1,
        "5s" => 2,
        "1m" => 10,
        "3m" => 11,
        "5m" => 12,
        "15m" => 13,
        "30m" => 14,
        "1h" => 20,
        "2h" => 21,
        "4h" => 22,
        "6h" => 23,
        "8h" => 24,
        "12h" => 25,
        "1d" => 30,
        "3d" => 31,
        "1w" => 32,
        "1M" => 33,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_timeframe_allowlist() {
        assert!(valid_pair("BTCUSDT"));
        assert!(valid_pair("BTC_USDT"));
        assert!(!valid_pair(""));
        assert!(!valid_pair(".."));
        assert!(!valid_pair("../../etc"));
        assert!(!valid_pair("a/b"));
        assert!(!valid_pair("a\\b"));
        assert!(!valid_pair("a b"));
        assert!(!valid_pair("btcusdt"));
        assert!(!valid_pair(".hidden"));

        assert!(valid_timeframe("1h"));
        assert!(valid_timeframe("15m"));
        assert!(valid_timeframe("1_2h"));
        assert!(!valid_timeframe(""));
        assert!(!valid_timeframe(".."));
        assert!(!valid_timeframe("1/h"));
        assert!(!valid_timeframe("1h "));
        assert!(!valid_timeframe("1H"));
    }

    #[test]
    fn timeframe_sorts_chronologically() {
        assert!(timeframe_order("1m") < timeframe_order("5m"));
        assert!(timeframe_order("1h") < timeframe_order("4h"));
        assert_eq!(timeframe_order("unknown"), 99);
    }
}
