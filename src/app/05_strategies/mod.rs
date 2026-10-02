pub mod aberration;
pub mod alma;
pub mod atrband;
pub mod bbmr;
pub mod bbrk;
pub mod build;
pub mod dema;
pub mod dualthrust;
pub mod ema;
pub mod ema_cross;
pub mod fiali;
pub mod fwma;
pub mod hma;
pub mod jma;
pub mod kama;
pub mod keltner;
pub mod linreg;
pub mod mcginley;
pub mod pwma;
pub mod rbreaker;
pub mod rsi2;
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
pub mod turtle;
pub mod vidya;
pub mod vwma;
pub mod wilders;
pub mod wma;
pub mod zlema;

pub use shared::{
    DayBar, PoleCount, Signal, aggregate_days, bars_per_year, candle_direction_filter,
    expand_f64_range, is_known_timeframe, price_cross_signal, rolling_highest, rolling_lowest,
    shift_right_1, supersmoother_3_pole, utc_day_index,
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

    /// 全量信号入口(high/low/time)。默认实现忽略多余输入, 行为与
    /// `generate_signals` 逐位一致 —— 老策略零改动; 日内/通道策略重写本方法。
    /// run_combo 只调本方法。
    #[allow(clippy::too_many_arguments)]
    fn generate_signals_full(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let _ = (high, low, time);
        self.generate_signals(open, close, volume, param_idx)
    }
}
