mod config;
mod strategy;
pub use config::BbrkBbConfig;
pub use strategy::BbrkBbStrategy;

/// BB中轨候选（用户指定对照组：sma基线 + ema/hma/zlema）。
/// 全部走 vector-ta `bollinger_bands` 的 matype 口径，零自实现偏差。
pub const BB_MA_NAMES: [&str; 4] = ["sma", "ema", "hma", "zlema"];
