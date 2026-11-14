//! 单 MA 策略的公共骨架。
//!
//! 「价格穿越单条均线」类策略(20 个 vector-ta 均线 + McGinley/VIDYA 两条自算均线)
//! 原本各有一份逐字相同的 [`Strategy`] 实现: 周期网格、`param_str` 前缀与
//! `generate_signals_full` 全部复制。这里只保留一份泛型实现,
//! 每个策略模块仅声明自己的 [`MaCalculator`](crate::strategies::single_ma::MaCalculator),
//! 行为与拆分前逐位一致。

use std::marker::PhantomData;

use super::{Signal, Strategy, price_cross_signal};
use crate::config::RangeParam;

/// 单 MA 策略的指标计算器。
pub trait MaCalculator {
    /// 报告 `strategy` 列使用的策略名。
    const NAME: &'static str;
    /// `param_str` 前缀(与历史报告保持逐字一致)。
    const KEY: &'static str;

    /// 计算整条均线; `period` 来自参数网格。
    ///
    /// # Errors
    /// 指标库拒绝该周期(过短/过长)时返回 [`crate::error::AppError::ParamSkipped`]。
    fn compute(close: &[f64], period: usize) -> crate::error::Result<Vec<f64>>;
}

/// 价格穿越单条均线的通用策略。
pub struct SingleMaStrategy<C: MaCalculator> {
    periods: Vec<usize>,
    _calc: PhantomData<fn() -> C>,
}

impl<C: MaCalculator> SingleMaStrategy<C> {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
    #[must_use]
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self {
            periods,
            _calc: PhantomData,
        }
    }
}

impl<C: MaCalculator> Strategy for SingleMaStrategy<C> {
    fn name(&self) -> &'static str {
        C::NAME
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("{}_{}", C::KEY, self.periods[idx])
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        let ma = C::compute(bars.close(), period)?;
        Ok(price_cross_signal(&ma, bars.close()))
    }
}
