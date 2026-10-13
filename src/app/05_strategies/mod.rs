pub mod aberration;
pub mod alma;
pub mod atma;
pub mod atrband;
pub mod bbmr;
pub mod bbrk;
pub mod bbrk_adx;
pub mod bbrk_atr;
pub mod bbrk_atr_ema;
pub mod bbrk_bb;
pub mod bbrk_cross;
pub mod bbrk_don;
pub mod bbrk_hma;
pub mod bbrk_hma_ema;
pub mod bbrk_hma_v1;
pub mod bbrk_hma_v1_sl;
pub mod bbrk_hma_v2;
pub mod bbrk_hma_v2_sl;
pub mod bbrk_hma_v3;
pub mod bbrk_hma_v3_sl;
pub mod bbrk_hma_v4;
pub mod bbrk_hma_v4_sl;
pub mod bbrk_hma_v5;
pub mod bbrk_hma_v5_sl;
pub mod bbrk_kdj;
pub mod bbrk_m2;
pub mod bbrk_m3;
pub mod bbrk_max;
pub mod bbrk_orth;
pub mod bbrk_pctb;
pub mod bbrk_rex;
pub mod bbrk_sma;
pub mod bbrk_sma_ema;
pub mod bbrk_str;
pub mod bbrk_trail;
pub mod bbrk_ttm;
pub mod bbrk_ttm_fire;
pub mod bbrk_vol;
pub mod bbrk_vol_ema;
pub mod bbrk_zlema_ema;
pub mod build;
pub mod dema;
pub mod dma;
pub mod dualthrust;
pub mod edma;
pub mod ehma;
pub mod ema;
pub mod ema_cross;
pub mod fiali;
pub mod frama;
pub mod fwma;
pub mod hma;
pub mod jma;
pub mod kama;
pub mod keltner;
pub mod linreg;
pub mod mabb_long;
pub mod mabb_short;
pub mod macd;
pub mod mcginley;
pub mod pwma;
pub mod rbreaker;
pub mod rsi2;
pub mod sgma;
mod shared;
pub mod sinwma;
pub mod sma;
pub mod sma_cd;
pub mod smma;
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
pub mod vwap;
pub mod vwap_trend;
pub mod vwma;
pub mod wilders;
pub mod wma;
pub mod zlema;

pub use shared::{
    CachedBands, DayBar, PoleCount, Signal, aggregate_days, bars_per_year, candle_direction_filter,
    expand_f64_range, is_known_timeframe, money_flow_index, price_cross_signal, rolling_highest,
    rolling_lowest, shift_right_1, supersmoother_3_pole, utc_day_index,
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

    /// 批量信号入口：一次算出 [start, end) 参数的全部信号（与 range 一一对应，
    /// 顺序保证），允许重参数策略在内部复用跨参数共享指标
    /// （如:**同σ的BB、与参数无关的ATR/量能前缀和/动量柱**）。
    /// 默认实现 = 按参数并行逐个调（与旧引擎循环等价）；重参数策略重写本方法做hoist。
    /// 引擎是唯一调用方（逐批调，内存预算逻辑不变）。
    #[allow(clippy::too_many_arguments)]
    fn generate_signals_batch(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        time: &[i64],
        range: std::ops::Range<usize>,
    ) -> crate::error::Result<Vec<Vec<Signal>>> {
        use rayon::prelude::*;
        range
            .into_par_iter()
            .map(|i| {
                self.generate_signals_full(open, high, low, close, volume, time, i)
            })
            .collect()
    }
}
