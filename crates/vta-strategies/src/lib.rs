//! # vta-strategies — 策略实现与参数网格
//!
//! 81 个策略各一个模块, 统一的 [`Strategy`] trait 与逐档 [`ParamOutcome`]
//! 契约; 参数类型与网格展开在 [`config`], 参数串编码在 [`params`]。
//!
//! 本 crate 只依赖 `vta-core`(引擎/信号/金额), 不依赖 CLI 与 Parquet 层。

/// 由 `StrategyConfig` 构建策略实例, 并做参数网格唯一性自检。
pub mod build;
pub mod config;
mod grid;
pub mod params;
mod registry;
/// 跨策略共用的信号类型、工具函数与周期年化表(不对外公开)。
mod shared;
/// 单 MA 策略的公共骨架: 泛型 `Strategy` 实现与指标计算器 trait。
pub mod single_ma;

use registry::strategy_table;

/// 由注册表生成全部策略模块: `pub mod` 与模块文档同源。
macro_rules! declare_strategy_modules {
    ($(($module:ident, $tag:literal, $variant:ident, $config_module:ident, $config:ident, $strategy:ident, $kind:ident)),* $(,)?) => {
        $(
            #[doc = concat!(
                "**", stringify!($variant), "** 策略(`type = \"", $tag, "\"`): 配置 `",
                stringify!($config), "`, 实现 `", stringify!($strategy), "`。"
            )]
            pub mod $module;
        )*
    };
}

strategy_table!(declare_strategy_modules);

pub use config::{
    F64Param, MAX_PERIOD_RANGE, MaGridCfg, PeriodParam, RangeParam, STRATEGY_TAGS, StrategyConfig,
    StrategySpec,
};
pub use params::ParamSet;
pub use shared::{
    CachedBands, DEFAULT_BB_PERIOD, DayBar, MAX_GRID_PREALLOC, PoleCount, aggregate_days,
    bars_per_year, candle_direction_filter, expand_f64_range, grid_prealloc, is_known_timeframe,
    money_flow_index, pct_b, price_cross_signal, rolling_highest, rolling_lowest, shift_right_1,
    supersmoother_3_pole, utc_day_index,
};
pub use sma_cd::SmaCdStrategy;

/// 单个参数档位的求值结果。
///
/// 存在的理由: "**这一档算不出来**"必须与"**这一档跑了但没信号**"在报告里
/// **可区分**。前者是 [`vta_core::RunStatus::Invalid`], 后者是
/// [`vta_core::RunStatus::NoSignals`]。在引入本类型之前, 66/67 个策略把
/// 算不出来的档位**静默**降级成"全程 Hold", 于是它在报告里长得和
/// "零波动低风险"的真实结论**一模一样**(都是 `trades=0 / pnl=0 / sharpe=0`),
/// 而 [`crate::Strategy`] 的契约明写"不做静默降级"。
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
/// [`vta_core::RunStatus::Invalid`] 并继续跑, 而不是静默降级。
#[must_use]
pub fn param_skipped(reason: impl std::fmt::Display) -> vta_core::error::AppError {
    vta_core::error::AppError::ParamSkipped {
        reason: reason.to_string(),
    }
}

pub use vta_core::{Bars, Signal};

/// 策略接口。
pub trait Strategy: Send + Sync {
    /// 策略名(报告 `strategy` 列)。
    #[must_use]
    fn name(&self) -> &'static str;
    /// 参数网格大小。
    #[must_use]
    fn param_count(&self) -> usize;
    /// 第 `idx` 组参数的结构化表示(文本 / JSON / 整数编码同源产出)。
    #[must_use]
    fn param_set(&self, idx: usize) -> ParamSet;

    /// 第 `idx` 组参数的文本形式(报告 `params` 列, 且必须网格内唯一)。
    ///
    /// 默认实现取 [`ParamSet::text`](crate::ParamSet::text); 需要文本的旧调用方
    /// (如缓存指纹)无需关心 JSON / 编码。
    #[must_use]
    fn param_str(&self, idx: usize) -> String {
        self.param_set(idx).into_text()
    }

    /// 生成信号。返回 `Err` 表示该参数/数据无法计算 (例如 VWMA 缺少等长 volume),
    /// **不做静默降级** —— 若属"预期内的不可计算", 请返回
    /// [`vta_core::error::AppError::ParamSkipped`] (经 [`param_skipped`] 构造),
    /// 引擎会把它记成 [`vta_core::RunStatus::Invalid`] 并**继续**跑下一档。
    fn generate_signals_full(
        &self,
        bars: Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>>;

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
    /// `AppError`(vta_core::error::AppError)** —— 契约违反是**调用方的 bug**,
    /// 故走错误通道而非 panic(panic 会让已经跑了几小时的整轮回测在最后一个
    /// 组合处崩溃, 且拿不到"哪个策略违反了契约"的可读信息)。
    fn generate_signals_batch(
        &self,
        bars: Bars<'_>,
        range: std::ops::Range<usize>,
    ) -> vta_core::error::Result<Vec<ParamOutcome>> {
        use rayon::prelude::*;
        range
            .into_par_iter()
            .map(|i| {
                match self.generate_signals_full(bars, i) {
                    Ok(s) => Ok(ParamOutcome::Signals(s)),
                    // "不可计算"是**预期内**的: 记为该档跳过, 不中断整轮。
                    Err(vta_core::error::AppError::ParamSkipped { reason }) => {
                        Ok(ParamOutcome::Skipped(reason))
                    }
                    // 其它错误是**真 bug**: 中断整轮, 让调用方拿到可读信息。
                    Err(e) => Err(e),
                }
            })
            .collect()
    }
}
