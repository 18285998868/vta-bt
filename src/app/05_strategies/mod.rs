/// **ABERRATION** 策略: 配置 [`AberrationConfig`], 实现 [`AberrationStrategy`]。
pub mod aberration;
/// **ALMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`AlmaStrategy`]。
pub mod alma;
/// **ATMA** 策略: 配置 [`AtmaConfig`], 实现 [`AtmaStrategy`]。
pub mod atma;
/// **ATRBAND** 策略: 配置 [`ATRBandConfig`], 实现 [`ATRBandStrategy`]。
pub mod atrband;
/// **BBMR** 策略: 配置 [`BBMRConfig`], 实现 [`BBMRStrategy`]。
pub mod bbmr;
/// **BBRK** 策略: 配置 [`BBRKConfig`], 实现 [`BBRKStrategy`]。
pub mod bbrk;
/// **BBRK ADX** 策略: 配置 [`BbrkAdxConfig`], 实现 [`BbrkAdxStrategy`]。
pub mod bbrk_adx;
/// **BBRK ATR** 策略: 配置 [`BbrkAtrConfig`], 实现 [`BbrkAtrStrategy`]。
pub mod bbrk_atr;
/// **BBRK ATR EMA** 策略: 配置 [`BbrkAtrEmaConfig`], 实现 [`BbrkAtrEmaStrategy`]。
pub mod bbrk_atr_ema;
/// **BBRK BB** 策略: 配置 [`BbrkBbConfig`], 实现 [`BbrkBbStrategy`]。
pub mod bbrk_bb;
/// **BBRK CROSS** 策略: 配置 [`BbrkCrossConfig`], 实现 [`Strategy`]。
pub mod bbrk_cross;
/// **BBRK DON** 策略: 配置 [`BbrkDonConfig`], 实现 [`BbrkDonStrategy`]。
pub mod bbrk_don;
/// **BBRK EMA** 策略: 配置 [`BbrkEmaConfig`], 实现 [`BbrkEmaStrategy`]。
pub mod bbrk_ema;
/// **BBRK HMA** 策略: 配置 [`BbrkHmaConfig`], 实现 [`BbrkHmaStrategy`]。
pub mod bbrk_hma;
/// **BBRK HMA EMA** 策略: 配置 [`BbrkHmaEmaConfig`], 实现 [`BbrkHmaEmaStrategy`]。
pub mod bbrk_hma_ema;
/// **BBRK HMA V1** 策略: 配置 [`BbrkHmaV1Config`], 实现 [`BbrkHmaV1Strategy`]。
pub mod bbrk_hma_v1;
/// **BBRK HMA V2** 策略: 配置 [`BbrkHmaV2Config`], 实现 [`BbrkHmaV2Strategy`]。
pub mod bbrk_hma_v2;
/// **BBRK HMA V3** 策略: 配置 [`BbrkHmaV3Config`], 实现 [`BbrkHmaV3Strategy`]。
pub mod bbrk_hma_v3;
/// **BBRK HMA V4** 策略: 配置 [`BbrkHmaV4Config`], 实现 [`BbrkHmaV4Strategy`]。
pub mod bbrk_hma_v4;
/// **BBRK HMA V5** 策略: 配置 [`BbrkHmaV5Config`], 实现 [`BbrkHmaV5Strategy`]。
pub mod bbrk_hma_v5;
/// **BBRK KDJ** 策略: 配置 [`BbrkKdjConfig`], 实现 [`BbrkKdjStrategy`]。
pub mod bbrk_kdj;
/// **BBRK M2** 策略: 配置 [`BbrkM2Config`], 实现 [`BbrkM2Strategy`]。
pub mod bbrk_m2;
/// **BBRK M3** 策略: 配置 [`BbrkM3Config`], 实现 [`BbrkM3Strategy`]。
pub mod bbrk_m3;
/// **BBRK MAX** 策略: 配置 [`BbrkMaxConfig`], 实现 [`BbrkMaxStrategy`]。
pub mod bbrk_max;
/// **BBRK ORTH** 策略: 配置 [`BbrkOrthConfig`], 实现 [`Strategy`]。
pub mod bbrk_orth;
/// **BBRK PCTB** 策略: 配置 [`BbrkPctbConfig`], 实现 [`BbrkPctbStrategy`]。
pub mod bbrk_pctb;
/// **BBRK REX** 策略: 配置 [`BbrkRexConfig`], 实现 [`BbrkRexStrategy`]。
pub mod bbrk_rex;
/// **BBRK SMA** 策略: 配置 [`BbrkSmaConfig`], 实现 [`BbrkSmaStrategy`]。
pub mod bbrk_sma;
/// **BBRK SMA EMA** 策略: 配置 [`BbrkSmaEmaConfig`], 实现 [`BbrkSmaEmaStrategy`]。
pub mod bbrk_sma_ema;
/// **BBRK STR** 策略: 配置 [`BbrkStrConfig`], 实现 [`BbrkStrStrategy`]。
pub mod bbrk_str;
/// **BBRK TRAIL** 策略: 配置 [`BbrkTrailConfig`], 实现 [`BbrkTrailStrategy`]。
pub mod bbrk_trail;
/// **BBRK TTM** 策略: 配置 [`BbrkTtmConfig`], 实现 [`BbrkTtmStrategy`]。
pub mod bbrk_ttm;
/// **BBRK TTM FIRE** 策略: 配置 [`BbrkTtmFireConfig`], 实现 [`BbrkTtmFireStrategy`]。
pub mod bbrk_ttm_fire;
/// **BBRK VOL** 策略: 配置 [`BbrkVolConfig`], 实现 [`BbrkVolStrategy`]。
pub mod bbrk_vol;
/// **BBRK VOL EMA** 策略: 配置 [`BbrkVolEmaConfig`], 实现 [`BbrkVolEmaStrategy`]。
pub mod bbrk_vol_ema;
/// **BBRK ZLEMA EMA** 策略: 配置 [`BbrkZlemaEmaConfig`], 实现 [`BbrkZlemaEmaStrategy`]。
pub mod bbrk_zlema_ema;
/// 由 `StrategyConfig` 构建策略实例, 并做参数网格唯一性自检。
pub mod build;
/// **CLENOW** 策略: 配置 [`ClenowConfig`], 实现 [`ClenowStrategy`]。
pub mod clenow;
/// **DEMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`DemaStrategy`]。
pub mod dema;
/// **DMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`DmaStrategy`]。
pub mod dma;
/// **DUALTHRUST** 策略: 配置 [`DualThrustConfig`], 实现 [`DualThrustStrategy`]。
pub mod dualthrust;
/// **EDMA** 策略: 配置 [`EdmaConfig`], 实现 [`EdmaStrategy`]。
pub mod edma;
/// **EHMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`EhmaStrategy`]。
pub mod ehma;
/// **EMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`EmaStrategy`]。
pub mod ema;
/// **EMA CROSS** 策略: 配置 [`EmaCrossConfig`], 实现 [`EmaCrossStrategy`]。
pub mod ema_cross;
/// **FIALI** 策略: 配置 [`FialiConfig`], 实现 [`FialiStrategy`]。
pub mod fiali;
/// **FRAMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`FramaStrategy`]。
pub mod frama;
/// **FWMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`FwmaStrategy`]。
pub mod fwma;
/// **HMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`HmaStrategy`]。
pub mod hma;
/// **JMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`JmaStrategy`]。
pub mod jma;
/// **KAMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`KamaStrategy`]。
pub mod kama;
/// **KELTNER** 策略: 配置 [`KeltnerConfig`], 实现 [`KeltnerStrategy`]。
pub mod keltner;
/// **LINREG** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`LinRegStrategy`]。
pub mod linreg;
/// **MA ATR** 策略: 配置 [`MaAtrConfig`], 实现 [`Strategy`]。
pub mod ma_atr;
/// **MA SLOPE ATR GATE** 策略: 配置 [`MaSlopeAtrGateConfig`], 实现 [`MaSlopeAtrGateStrategy`]。
pub mod ma_slope_atr_gate;
/// **MABB LONG** 策略: 配置 [`MabbLongConfig`], 实现 [`Strategy`]。
pub mod mabb_long;
/// **MABB SHORT** 策略: 配置 [`MabbShortConfig`], 实现 [`MabbShortStrategy`]。
pub mod mabb_short;
/// **MACD** 策略: 配置 [`MacdConfig`], 实现 [`MacdStrategy`]。
pub mod macd;
/// **MCGINLEY** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`McGinleyStrategy`]。
pub mod mcginley;
/// **PWMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`PwmaStrategy`]。
pub mod pwma;
/// **RBREAKER** 策略: 配置 [`RBreakerConfig`], 实现 [`RBreakerStrategy`]。
pub mod rbreaker;
/// **RSI2** 策略: 配置 [`RSI2Config`], 实现 [`RSI2Strategy`]。
pub mod rsi2;
/// **SGMA** 策略: 配置 [`SgmaConfig`], 实现 [`SgmaStrategy`]。
pub mod sgma;
/// 跨策略共用的信号类型、工具函数与周期年化表(不对外公开)。
mod shared;
/// **SINWMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`SinWmaStrategy`]。
pub mod sinwma;
/// **SMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`SmaStrategy`]。
pub mod sma;
/// **SMA CD** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`SmaCdStrategy`]。
pub mod sma_cd;
/// **SMMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`SmmaStrategy`]。
pub mod smma;
/// **SSF** 策略: 配置 [`SsfConfig`], 实现 [`SsfStrategy`]。
pub mod ssf;
/// **SSF CD** 策略: 配置 [`SsfCdConfig`], 实现 [`SsfCdStrategy`]。
pub mod ssf_cd;
/// **SSF ST** 策略: 配置 [`SsfStConfig`], 实现 [`SsfStStrategy`]。
pub mod ssf_st;
/// **SSF ST CD** 策略: 配置 [`SsfStCdConfig`], 实现 [`SsfStCdStrategy`]。
pub mod ssf_st_cd;
/// **SWINGFRAC** 摆动点突破策略: 配置 [`SwingFractalConfig`], 实现 [`SwingFractalStrategy`]。
pub mod swing_fractal;
/// **SWMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`SwmaStrategy`]。
pub mod swma;
/// **TEMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`TemaStrategy`]。
pub mod tema;
/// **TILSON** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`TilsonStrategy`]。
pub mod tilson;
/// **TRIMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`TrimaStrategy`]。
pub mod trima;
/// **TURTLE** 策略: 配置 [`TurtleConfig`], 实现 [`TurtleStrategy`]。
pub mod turtle;
/// **VIDYA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`VidyaStrategy`]。
pub mod vidya;
/// **VWAP** 策略: 配置 [`VwapConfig`], 实现 [`VwapStrategy`]。
pub mod vwap;
/// **VWAP TREND** 策略: 配置 [`VwapTrendConfig`], 实现 [`VwapTrendStrategy`]。
pub mod vwap_trend;
/// **VWMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`VwmaStrategy`]。
pub mod vwma;
/// **WILDERS** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`WildersStrategy`]。
pub mod wilders;
/// **WMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`WmaStrategy`]。
pub mod wma;
/// **ZLEMA** 均线策略: 配置 [`MaGridCfg`](crate::config::MaGridCfg), 实现 [`ZlemaStrategy`]。
pub mod zlema;

pub use shared::{
    CachedBands, DEFAULT_BB_PERIOD, DayBar, MAX_GRID_PREALLOC, PoleCount, Signal, StopLines,
    aggregate_days, bars_per_year, candle_direction_filter, expand_f64_range, grid_prealloc,
    is_known_timeframe, money_flow_index, pct_b, price_cross_signal, rolling_highest,
    rolling_lowest, shift_right_1, supersmoother_3_pole, utc_day_index,
};
pub use sma_cd::SmaCdStrategy;

/// 单个参数档位的求值结果。
///
/// 存在的理由: "**这一档算不出来**"必须与"**这一档跑了但没信号**"在报告里
/// **可区分**。前者是 [`crate::app::RunStatus::Invalid`], 后者是
/// [`crate::app::RunStatus::NoSignals`]。在引入本类型之前, 66/67 个策略把
/// 算不出来的档位**静默**降级成"全程 Hold", 于是它在报告里长得和
/// "零波动低风险"的真实结论**一模一样**(都是 `trades=0 / pnl=0 / sharpe=0`),
/// 而 [`crate::strategies::Strategy`] 的契约明写"不做静默降级"。
#[derive(Debug, Clone, PartialEq)]
pub enum ParamOutcome {
    /// 算出来了: 该档位的逐 bar 信号。
    Signals(Vec<Signal>),
    /// 该档位**不可计算**(数据/参数不足)。`reason` 是可读原因, 会进日志。
    Skipped(String),
}

impl ParamOutcome {
    /// 取信号; 不可计算时返回 `None`。
    #[must_use]
    pub fn signals(&self) -> Option<&[Signal]> {
        match self {
            Self::Signals(s) => Some(s),
            Self::Skipped(_) => None,
        }
    }
}

/// 构造"该参数档位不可计算"的错误。
///
/// 供策略侧统一使用: 算不出指标时返回它, 引擎会把它记成
/// [`crate::app::RunStatus::Invalid`] 并继续跑, 而不是静默降级。
#[must_use]
pub fn param_skipped(reason: impl std::fmt::Display) -> crate::error::AppError {
    crate::error::AppError::ParamSkipped {
        reason: reason.to_string(),
    }
}

/// 策略接口。
pub trait Strategy: Send + Sync {
    /// 策略名(报告 `strategy` 列)。
    #[must_use]
    fn name(&self) -> &'static str;
    /// 参数网格大小。
    #[must_use]
    fn param_count(&self) -> usize;
    /// 第 `idx` 组参数的文本形式(报告 `params` 列, 且必须网格内唯一)。
    #[must_use]
    fn param_str(&self, idx: usize) -> String;

    /// 生成信号。返回 `Err` 表示该参数/数据无法计算 (例如 VWMA 缺少等长 volume),
    /// **不做静默降级** —— 若属"预期内的不可计算", 请返回
    /// [`crate::error::AppError::ParamSkipped`] (经 [`param_skipped`] 构造),
    /// 引擎会把它记成 [`crate::app::RunStatus::Invalid`] 并**继续**跑下一档。
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

    /// 生成**策略止损价格线**(可选)。
    ///
    /// 默认不提供。实现本方法的策略负责给出逐 bar 的止损价, 引擎会在盘中
    /// `low/high` 触发时按 `StopLines` 语义成交, 平仓原因记为
    /// [`crate::engine::ExitReason::StrategyStop`]。返回值长度必须覆盖
    /// `generate_signals_full` 的信号长度; `None` / 非法价格表示该 bar 不设止损。
    ///
    /// **无前视**: `long[i]` / `short[i]` 只能使用第 `i` 根 bar 开始前已知的信息。
    // 参数顺序与 `generate_signals_full` 完全一致; 顺序契约由同一组测试锁定。
    #[allow(clippy::too_many_arguments)]
    fn generate_stop_lines(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Option<StopLines>> {
        let _ = (open, high, low, close, volume, time, param_idx);
        Ok(None)
    }

    /// 批量信号入口：一次算出 [start, end) 参数的全部信号（与 range 一一对应，
    /// 顺序保证），允许重参数策略在内部复用跨参数共享指标
    /// （如:**同σ的BB、与参数无关的ATR/量能前缀和/动量柱**）。
    /// 默认实现 = 按参数并行逐个调 [`Strategy::generate_signals_full`]（与旧引擎
    /// 循环等价）；重参数策略重写本方法做 hoist。
    /// 引擎是唯一调用方（逐批调，内存预算逻辑不变）。
    ///
    /// **逐档独立**: 某一档不可计算时, 该档返回 [`ParamOutcome::Skipped`],
    /// **其余档照常计算** —— 绝不因为一档参数就中断整轮。
    ///
    /// **契约**: 返回长度必须等于 `range` 长度, 否则 `run_combo` 会**返回
    /// [`AppError`](crate::error::AppError)** —— 契约违反是**调用方的 bug**,
    /// 故走错误通道而非 panic(panic 会让已经跑了几小时的整轮回测在最后一个
    /// 组合处崩溃, 且拿不到"哪个策略违反了契约"的可读信息)。
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
    ) -> crate::error::Result<Vec<ParamOutcome>> {
        use rayon::prelude::*;
        range
            .into_par_iter()
            .map(|i| {
                match self.generate_signals_full(open, high, low, close, volume, time, i) {
                    Ok(s) => Ok(ParamOutcome::Signals(s)),
                    // "不可计算"是**预期内**的: 记为该档跳过, 不中断整轮。
                    Err(crate::error::AppError::ParamSkipped { reason }) => {
                        Ok(ParamOutcome::Skipped(reason))
                    }
                    // 其它错误是**真 bug**: 中断整轮, 让调用方拿到可读信息。
                    Err(e) => Err(e),
                }
            })
            .collect()
    }
}
