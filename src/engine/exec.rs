//! 成交执行假设与持仓方向。

use std::num::NonZeroUsize;

use crate::strategies::Signal;

/// 成交执行假设。`delay_bars = 1` 为推荐口径(信号次根 fill bar 成交,
/// `fill_close=false` 取开盘, `fill_close=true` 取收盘)。费率按单边名义扣除。
///
/// **`delay_bars` 恒 ≥ 1**: 信号次根 fill bar 成交。`delay_bars = 0`(信号当根收盘成交)
/// 属前视乐观口径, 已在**引擎层彻底移除** —— 字段是 [`NonZeroUsize`], 类型上无法
/// 构造 0。费率按单边名义扣除。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecConfig {
    /// 信号到成交的 bar 间隔, 恒 ≥ 1(类型保证)。1 = 次根 fill bar。
    pub delay_bars: NonZeroUsize,
    /// 单边佣金率(= bps / 10000)。**必须 >= 0 且有限**; 构造期与引擎入口
    /// ([`simulate`](super::simulate) / [`simulate_detailed`](super::simulate_detailed)) 都会校验。
    pub fee_rate: f64,
    /// 单边逆向滑点率(= bps / 10000)。约束同 [`Self::fee_rate`]。
    pub slip_rate: f64,
    /// fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
}

impl Default for ExecConfig {
    /// 推荐口径: 次根 fill bar 成交(开盘价)、无成本。
    fn default() -> Self {
        Self {
            delay_bars: NonZeroUsize::MIN,
            fee_rate: 0.0,
            slip_rate: 0.0,
            fill_close: false,
        }
    }
}

impl ExecConfig {
    /// 构造执行配置。`delay_bars = 0`、负/非有限的费率或滑点、或换算回 bps
    /// 会溢出的费率直接拒绝。
    ///
    /// # Errors
    /// `delay_bars == 0`、`fee_bps`/`slippage_bps` 为负或 `NaN`/`Inf`、或
    /// 派生 bps 非有限时返回 [`AppError::Msg`](crate::error::AppError::Msg)。
    ///
    /// 配置路径在反序列化期已用 newtype 保证非负有限; 这里再校验一次, 是因为
    /// [`ExecConfig`] 是**公开的引擎 API**, 调用方可以绕过
    /// [`crate::config::BacktestConfig`] 直接构造。负费率会让每笔交易"倒赚钱",
    /// `NaN` 会污染整条权益曲线 —— 都属于必须在入口拒绝而不是静默传播的输入。
    pub fn try_from_bps_fill(
        delay_bars: usize,
        fee_bps: f64,
        slippage_bps: f64,
        fill_close: bool,
    ) -> crate::error::Result<Self> {
        let delay_bars = NonZeroUsize::new(delay_bars).ok_or_else(|| {
            crate::error::AppError::Msg(
                "delay_bars 必须 >= 1: 信号当根成交为前视乐观口径, 已移除".to_string(),
            )
        })?;
        if !fee_bps.is_finite() || fee_bps < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "fee_bps 必须 >= 0 且为有限值, 当前为 {fee_bps}"
            )));
        }
        if !slippage_bps.is_finite() || slippage_bps < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "slippage_bps 必须 >= 0 且为有限值, 当前为 {slippage_bps}"
            )));
        }
        let config = Self {
            delay_bars,
            fee_rate: fee_bps / 10_000.0,
            slip_rate: slippage_bps / 10_000.0,
            fill_close,
        };
        // 费率本身有限不代表 `rate * 10_000` 有限; 后者会写进报告的 bps 列。
        config.validate_rates()?;
        Ok(config)
    }

    /// [`Self::try_from_bps_fill`] 的 infallible 版本(固定 `fill_close = false`)。
    ///
    /// # Panics
    /// `delay_bars == 0`、负/非有限的费率或滑点时 panic。
    ///
    /// 这个 `expect` 是**故意的"bug 探测器"**(`err-expect-bugs-only`):
    /// 这些值不是用户输入错误 —— 配置路径在**反序列化期**就拒了
    /// (`BacktestConfig::try_from`), 能走到这里的非法值一定是代码写错了,
    /// 应当立刻炸掉而不是返回一个"前视乐观/倒赚手续费"的执行口径。
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn from_bps_fill(
        delay_bars: usize,
        fee_bps: f64,
        slippage_bps: f64,
        fill_close: bool,
    ) -> Self {
        Self::try_from_bps_fill(delay_bars, fee_bps, slippage_bps, fill_close)
            .expect("执行参数必须合法: delay>=1 且 fee/slip 非负有限")
    }

    /// 引擎入口的最终校验: 即使调用方绕过构造函数直接写公开字段, 也不能把
    /// 负费率 / `NaN` 带进账本。
    pub(super) fn validate_rates(&self) -> crate::error::Result<()> {
        if !self.fee_rate.is_finite() || self.fee_rate < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "fee_rate 必须 >= 0 且为有限值, 当前为 {}",
                self.fee_rate
            )));
        }
        if !self.slip_rate.is_finite() || self.slip_rate < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "slip_rate 必须 >= 0 且为有限值, 当前为 {}",
                self.slip_rate
            )));
        }
        // `apply_exec` 会写回 `rate * 10_000`; 该派生值也必须是有限数。
        if !(self.fee_rate * 10_000.0).is_finite() {
            return Err(crate::error::AppError::Msg(
                "fee_rate 过大: 换算回 fee_bps 时溢出".to_string(),
            ));
        }
        if !(self.slip_rate * 10_000.0).is_finite() {
            return Err(crate::error::AppError::Msg(
                "slip_rate 过大: 换算回 slippage_bps 时溢出".to_string(),
            ));
        }
        Ok(())
    }

    /// [`Self::from_bps_fill`] 的 `fill_close = false` 简写。
    ///
    /// # Panics
    /// 同 [`Self::from_bps_fill`]。
    #[must_use]
    pub fn from_bps(delay_bars: usize, fee_bps: f64, slippage_bps: f64) -> Self {
        Self::from_bps_fill(delay_bars, fee_bps, slippage_bps, false)
    }

    /// 信号到成交的 bar 间隔(恒 ≥1)。
    #[must_use]
    pub fn delay(&self) -> usize {
        self.delay_bars.get()
    }

    /// 信号到成交的 bar 间隔, 收窄为报告列用的 `u32`。
    ///
    /// **不静默截断**: `usize → u32` 的 `as` 转换在 `delay > u32::MAX` 时会截成 0,
    /// 而 0 恰是本项目明令禁止的"前视乐观"标记值 —— 报告口径会与执行口径**静默
    /// 不一致**。这里改为饱和到 `u32::MAX`, 宁可报一个明显偏大的数, 也不报 0。
    #[must_use]
    pub fn delay_u32(&self) -> u32 {
        u32::try_from(self.delay_bars.get()).unwrap_or(u32::MAX)
    }
}

/// 持仓/订单方向。
///
/// 原实现用 `i8` 的 `+1/-1/0`, 但 `i8` 能表示 `2`、`-2`、`127` 等**引擎无法处理**
/// 的值: `fill_price(side==1)` 之外的任何值都走"卖出"分支, `cur_pos != 1 && != -1`
/// 的分支则被当作平仓 —— 两处都会静默算错而不报错。enum 让非法方向**不可表示**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// 无持仓。
    #[default]
    Flat,
    /// 多头。
    Long,
    /// 空头。
    Short,
}

impl Side {
    /// 符号: Long = +1, Short = -1, Flat = 0。
    #[must_use]
    pub fn sign(self) -> i8 {
        match self {
            Self::Flat => 0,
            Self::Long => 1,
            Self::Short => -1,
        }
    }

    /// 由 `+1` / `-1` / 其他 还原方向。
    #[must_use]
    pub fn from_sign(s: i8) -> Self {
        match s {
            1 => Self::Long,
            -1 => Self::Short,
            _ => Self::Flat,
        }
    }

    /// 反向(用于平仓: 取当前持仓的相反方向下单)。
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::Flat => Self::Flat,
            Self::Long => Self::Short,
            Self::Short => Self::Long,
        }
    }

    /// 是否处于持仓(`Long` 或 `Short`)。
    #[must_use]
    pub fn is_open(self) -> bool {
        self != Self::Flat
    }

    /// 报告/落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
            Self::Flat => "FLAT",
        }
    }

    /// 由信号推出的持仓方向。
    #[must_use]
    pub fn from_signal(s: Signal) -> Self {
        match s {
            Signal::Buy => Self::Long,
            Signal::Sell => Self::Short,
            Signal::Close | Signal::Hold | Signal::ExitLong | Signal::ExitShort => Self::Flat,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
