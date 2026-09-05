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

pub use shared::{Signal, bars_per_year, price_cross_signal};

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn param_count(&self) -> usize;
    fn param_str(&self, idx: usize) -> String;
    fn generate_signals(&self, close: &[f64], volume: &[f64], param_idx: usize) -> Vec<Signal>;
}
