mod config;
mod strategy;
pub use config::MaCrossConfig;
pub use strategy::{MaCrossStrategy, ma_cross_signals};

/// `ma_cross.ma` 的冻结名单, 与 `scripts/backtest/vbt-双均线交叉.py` 的
/// `MA_TYPES` 一致。
pub const MA_CROSS_NAMES: [&str; 3] = ["sma", "ema", "hma"];
