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
pub mod bbrk_ema;
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
pub mod ma_atr;
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

/// 策略接口。
///
/// **信号生成只有一条路径**: [`Strategy::generate_signals_full`]。历史上曾并存
/// `generate_signals`(close-only, required) / `generate_signals_full`(默认桥接前
/// 者) / `generate_signals_batch`(默认并行调 full) 三个入口, 而引擎只经 batch 调
/// full —— 于是 48 个重写 full 的策略同时维护了一份**永不执行**的
/// `generate_signals`, 且两者语义分叉(例如 FRAMA 的 close-only 版用
/// `close` 充当 high/low, 与 full 的真实 high/low 结果不同)。任何走错入口的
/// 调用方都会静默拿到错误信号。现已删除 close-only 入口。
pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn param_count(&self) -> usize;
    fn param_str(&self, idx: usize) -> String;

    /// 生成信号。返回 `Err` 表示该参数/数据无法计算 (例如 VWMA 缺少等长 volume),
    /// 不做静默降级。
    // 参数全为 `&[f64]`, 编译器无法校验顺序 —— 顺序契约由
    // `test_full_arg_order_contract` 显式锁定。
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
    ) -> crate::error::Result<Vec<Signal>>;

    /// 批量信号入口：一次算出 [start, end) 参数的全部信号（与 range 一一对应，
    /// 顺序保证），允许重参数策略在内部复用跨参数共享指标
    /// （如:**同σ的BB、与参数无关的ATR/量能前缀和/动量柱**）。
    /// 默认实现 = 按参数并行逐个调 [`Strategy::generate_signals_full`]（与旧引擎
    /// 循环等价）；重参数策略重写本方法做 hoist。
    /// 引擎是唯一调用方（逐批调，内存预算逻辑不变）。
    ///
    /// **契约**: 返回长度必须等于 `range` 长度, 否则 `run_combo` 会 panic。
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
            .map(|i| self.generate_signals_full(open, high, low, close, volume, time, i))
            .collect()
    }
}
