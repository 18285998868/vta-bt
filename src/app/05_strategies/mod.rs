pub mod alma;
pub mod build;
pub mod dema;
pub mod ema;
pub mod fwma;
pub mod hma;
pub mod jma;
pub mod kama;
pub mod linreg;
pub mod mcginley;
pub mod pwma;
mod shared;
pub mod sinwma;
pub mod sma;
pub mod sma_cd;
pub mod ssf;
pub mod ssf_cd;
pub mod ssf_st;
pub mod ssf_st_cd;
pub mod swma;
pub mod tema;
pub mod tilson;
pub mod trima;
pub mod vidya;
pub mod vwma;
pub mod wilders;
pub mod wma;
pub mod zlema;

pub use shared::{
    PoleCount, Signal, bars_per_year, candle_direction_filter, is_known_timeframe,
    price_cross_signal, supersmoother_3_pole,
};
pub use sma_cd::SmaCdStrategy;

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn param_count(&self) -> usize;
    fn param_str(&self, idx: usize) -> String;
    /// 生成信号。返回 Err 表示该参数/数据无法计算 (例如 VWMA 缺少等长 volume), 不做静默降级。
    fn generate_signals(
        &self,
        open: &[f64],
        close: &[f64],
        volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>>;
}
