mod config;
mod strategy;
pub use config::BbrkMaxConfig;
pub use strategy::BbrkMaxStrategy;

/// regime门候选均线（全部 period 单参口径，按平滑/滞后大致排序）。
/// 与 `bbrk_sma`/`bbrk_hma` 同决策逻辑，只换 regime 线。
pub const MA_NAMES: [&str; 12] = [
    "sma", "wma", "ema", "wilders", "dema", "tema", "trima", "hma", "zlema", "kama", "alma",
    "linreg",
];
