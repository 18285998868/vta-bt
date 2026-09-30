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
pub mod ssf;
pub mod ssf_st;
pub mod swma;
pub mod tema;
pub mod tilson;
pub mod trima;
pub mod vidya;
pub mod vwma;
pub mod wilders;
pub mod wma;
pub mod zlema;

pub use shared::{Signal, bars_per_year, is_known_timeframe, price_cross_signal};

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn param_count(&self) -> usize;
    fn param_str(&self, idx: usize) -> String;
    /// 生成信号。返回 Err 表示该参数/数据无法计算 (例如 VWMA 缺少等长 volume), 不做静默降级。
    fn generate_signals(
        &self,
        close: &[f64],
        volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>>;
}
