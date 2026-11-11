//! 共享工具：入参白名单与目录探测。

use std::path::Path;

/// 交易对允许字符集（大写字母 / 数字 / `_`）。
const PAIR_ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
/// 周期允许字符集（字母 / 数字 / `_`）。
///
/// 含大写字母：dukascopy 的周期写作 `M1` / `H1` / `D1`。
/// 本函数只管**安全**（字符集 + 长度，防目录穿越），不管**合法**
/// （该周期是否被数据源支持）——合法性由对应 reader 裁定，能给出
/// "该源支持哪些周期"的准确信息。
const TIMEFRAME_ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";

/// 交易对最大长度（防御超长路径分量）。
const MAX_PAIR_LEN: usize = 32;
/// 周期最大长度（防御超长路径分量）。
const MAX_TIMEFRAME_LEN: usize = 16;

/// 字符集 + 长度双重白名单校验。
fn is_allowed(s: &str, allowed: &str, max_len: usize) -> bool {
    !s.is_empty() && s.len() <= max_len && s.bytes().all(|b| allowed.contains(b as char))
}

/// 交易对白名单校验（大写字母/数字/`_`，防目录穿越）。
pub fn valid_pair(s: &str) -> bool {
    is_allowed(s, PAIR_ALLOWED, MAX_PAIR_LEN)
}

/// 周期白名单校验（字母/数字/`_`，防目录穿越）。
pub fn valid_timeframe(s: &str) -> bool {
    is_allowed(s, TIMEFRAME_ALLOWED, MAX_TIMEFRAME_LEN)
}

/// 目录下是否存在 kline zip 文件。
///
/// 目录不可读时按 `false` 处理（调用方已确认目录存在，此处只做"有没有数据"的
/// 辅助判断，读不到就当没有数据，不向上抛错）。
pub fn has_kline_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("zip"))
        })
        .unwrap_or(false)
}

/// 递归查找目录下**第一个**具有指定扩展名的文件（找到即停）。
///
/// binance 的 zip 直接躺在周期目录里（用 [`has_kline_files`] 即可），
/// dukascopy 的 `.bi5` 却嵌在 `年/月/日` 多层目录里，故需要递归。
/// 目录不可读时按 `false` 处理。
pub fn has_data_files(dir: &Path, ext: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if has_data_files(&path, ext) {
                return true;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            return true;
        }
    }
    false
}

/// 周期排序权重（未知周期沉底）。
pub fn timeframe_order(tf: &str) -> u32 {
    match tf {
        "1s" => 1,
        "5s" => 2,
        "1m" | "M1" => 10,
        "3m" => 11,
        "5m" => 12,
        "15m" => 13,
        "30m" => 14,
        "1h" | "H1" => 20,
        "2h" => 21,
        "4h" => 22,
        "6h" => 23,
        "8h" => 24,
        "12h" => 25,
        "1d" | "D1" => 30,
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
        // 大写允许：dukascopy 的周期写作 M1 / H1 / D1。
        // （本函数只管"安全"，不管"合法"——合法性由数据源裁定。）
        assert!(valid_timeframe("M1"));
        assert!(valid_timeframe("H1"));
        assert!(valid_timeframe("D1"));
        assert!(valid_timeframe("1H"));
    }

    #[test]
    fn rejects_overlong_input() {
        assert!(!valid_pair(&"A".repeat(MAX_PAIR_LEN + 1)));
        assert!(valid_pair(&"A".repeat(MAX_PAIR_LEN)));
        assert!(!valid_timeframe(&"a".repeat(MAX_TIMEFRAME_LEN + 1)));
        assert!(valid_timeframe(&"a".repeat(MAX_TIMEFRAME_LEN)));
    }

    #[test]
    fn timeframe_sorts_chronologically() {
        assert!(timeframe_order("1m") < timeframe_order("5m"));
        assert!(timeframe_order("1h") < timeframe_order("4h"));
        assert_eq!(timeframe_order("unknown"), 99);
    }

    #[test]
    fn has_kline_files_detects_zip() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(!has_kline_files(dir.path()));
        std::fs::write(dir.path().join("BTCUSDT-1h.zip"), b"x").unwrap();
        assert!(has_kline_files(dir.path()));
        // 不可读目录按 false 处理，不 panic
        assert!(!has_kline_files(Path::new("/definitely/not/here")));
    }
}
