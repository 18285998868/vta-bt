use std::num::NonZeroUsize;

use rayon::prelude::*;
use serde::Deserialize;

use crate::strategies::{Signal, StopLines};

/// 单次求值的全部回测指标。
///
/// **单位约定**: 带 `_pct` 的字段是百分数(15.0 = 15%), `total_return` /
/// `win_rate` 是小数(0.15 = 15%)。落盘时按 [`output::COLUMNS`](crate::output)
/// 声明的单位再折算一次, 该表由测试锁定与 schema 一致。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SummaryMetrics {
    /// 总收益率(百分数, 15.0 = 15%)。
    pub total_return_pct: f64,
    /// 总收益率(小数)。`= total_return_pct / 100`。
    pub total_return: f64,
    /// 年化夏普(逐 bar 对数收益, ddof=1 × √bars_per_year)。
    pub sharpe_ratio: f64,
    /// 下行偏差口径, 分母为**下行 bar 数**的样本方差(ddof=1), 与 `sharpe_ratio`
    /// 同一自由度约定, 两者可并列比较。
    pub sortino_ratio: f64,
    /// 最大回撤(百分数)。
    pub max_drawdown_pct: f64,
    /// 盈利笔数占**已决笔数**(盈利 + 亏损, 不含保本)的比例(百分数)。
    ///
    /// 保本(`pnl == 0`)既不算赢也不算亏, 故不进入分母 —— 否则"零盈亏"
    /// 会被读成一次失败交易, 系统性拉低胜率。
    pub win_rate: f64,
    /// 总盈利 / 总亏损。无亏损时取总盈利。
    pub profit_factor: f64,
    /// CAGR / 最大回撤。无回撤时为 0。
    pub calmar_ratio: f64,
    /// 已平仓交易笔数(往返各计一次)。
    pub num_trades: u32,
    /// 因固定百分比/ATR 止损离场的笔数。
    pub stop_loss_exits: u32,
    /// 因百分比/ATR 追踪止损或保本止损离场的笔数。
    pub trailing_stop_exits: u32,
    /// 因策略提供的止损价格线离场的笔数。
    pub strategy_stop_exits: u32,
    /// 盈利笔的平均 pnl(货币; 定点十进制)。
    pub avg_win: crate::money::Amount,
    /// 亏损笔的平均亏损**绝对值**(货币, 正数; 定点十进制)。
    pub avg_loss: crate::money::Amount,
    /// 逐笔 `pnl_pct` 的均值(小数)。
    ///
    /// **毛口径**: `pnl_pct` 由已含滑点的成交价算出, 但**不含佣金**。
    /// 与 [`Self::fixed_expectancy_bps`] 的差就是佣金拖累。
    pub expectancy: f64,
    /// 最长连续盈利笔数。
    ///
    /// 保本笔**不计入也不打断**连续段(它不构成一次盈利或亏损)。
    pub consecutive_wins: u32,
    /// 最长连亏笔数。
    ///
    /// 保本笔**不计入也不打断**连续段: 否则"零盈亏"会被当作一次亏损,
    /// 把连亏段人为拉长。
    pub consecutive_losses: u32,
    /// 末根权益(货币; 定点十进制)。空头浮空时可为负。
    pub final_equity: crate::money::Amount,
    /// 年化波动率(百分数)。
    pub volatility: f64,
    /// 总收益 / 最大回撤。
    pub recovery_factor: f64,
    /// 平均持仓 bar 数。
    pub avg_hold_bars: f64,
    /// 平均最大不利偏移(价格差)。
    pub avg_mae: f64,
    /// 平均最大有利偏移(价格差)。
    pub avg_mfe: f64,
    /// `avg_mfe / avg_mae`。avg_mae=0 时为 0。
    pub edge_ratio: f64,
    /// 平均盈利 / 平均亏损。
    pub wl_ratio: f64,
    /// 逐笔 pnl 的样本偏度(Fisher-Pearson 小样本校正; 正态=0)。
    pub skewness: f64,
    /// 逐笔 pnl 的超额峰度(Fisher 小样本校正; 正态=0; 仅 `num_trades > 3` 时计算)。
    pub kurtosis: f64,
    /// SQN = √n · mean/stdev (Van Thorp 1998)。**仅在 `num_trades` >= 30 时有意义**
    /// (Van Thorp 建议门槛); n 越大 SQN 越高是统计显著性的体现, 非过拟合信号。
    /// 判读时必须与 `num_trades` 配对。
    pub sqn: f64,
    /// 前后半段 profit factor 的比值(≤1)。样本 <20 时为 0。
    pub pf_stability: f64,
    /// 连盈连亏段数的 runs 检验 z 值。
    pub z_score: f64,
    /// runs 检验双尾 p 值。
    pub p_value: f64,
    /// fixed 腿(固定名义、不复利)总盈亏(货币; 定点十进制)。
    pub total_fixed_pnl: crate::money::Amount,
    /// 年化成交笔数(往返次数)。信号抖动的主要预警指标:
    /// 平均持仓不足 5 根 bar 时, 该值会高到成本必然吞掉全部毛利。
    pub turnover_per_year: f64,
    /// 执行口径: 信号到成交的 bar 间隔(恒 ≥1)。
    pub delay_bars: u32,
    /// 执行口径: 单边佣金(bp)。
    pub fee_bps: f64,
    /// 执行口径: 单边逆向滑点(bp)。
    pub slippage_bps: f64,
    /// fixed 腿总收益率(小数, 固定名义不复利)。研究报告引用此值, 而非复利 total_return。
    pub fixed_total_return: f64,
    /// fixed 腿年化夏普(与 sharpe_ratio 同公式, 曲线为 fixed 盯市)。
    pub fixed_sharpe_ratio: f64,
    /// fixed 腿最大回撤(百分数)。
    pub fixed_max_drawdown_pct: f64,
    /// **净期望**(bp, 以初始资金计): 固定名义腿的逐笔净盈亏均值,
    /// **含双边佣金与滑点**。与 [`Self::expectancy`] 相减即成本拖累
    /// (约等于两倍单边费率)。C6 成本分析的直接输入。
    pub fixed_expectancy_bps: f64,
    /// fixed 腿 profit factor。
    pub fixed_profit_factor: f64,
}

impl SummaryMetrics {
    /// 回填执行口径字段(`delay_bars` / `fee_bps` / `slippage_bps`)。
    ///
    /// 这三列描述的是**本次运行的执行假设**, 不是从交易序列算出的指标。
    /// 引擎存在多条返回路径(正常计算、数据不足提前返回、报告层构造的
    /// `invalid` 行), 若各写一份就容易出现"同一列有的行是 0、有的行是真实值"。
    /// 故收敛到这一个方法: 凡是产出 `SummaryMetrics` 的路径都必须调用它。
    pub fn apply_exec(&mut self, exec: &ExecConfig) {
        self.delay_bars = exec.delay_u32();
        self.fee_bps = exec.fee_rate * 10_000.0;
        self.slippage_bps = exec.slip_rate * 10_000.0;
    }
}

/// ATR 止损规则。
///
/// `mult` 是 ATR 倍数; `trailing=false` 为入场时固定的 ATR 止损,
/// `trailing=true` 为跟随有利极值移动的 ATR 追踪止损。ATR 使用 Wilder
/// 平滑, 入场时取**上一根已收盘 bar** 的 ATR(收盘成交取入场 bar 的 ATR),
/// 避免用入场后才走完的行情计算初始止损。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtrStopConfig {
    /// ATR 周期, 必须 >= 1。
    pub period: usize,
    /// ATR 倍数, 必须 > 0 且有限。
    pub mult: f64,
    /// 是否追踪有利极值。
    #[serde(default)]
    pub trailing: bool,
}

/// 保本止损规则。
///
/// 收盘价相对入场价的有利幅度达到 `trigger_pct` 后, 把止损移到入场价
/// 加减 `offset_pct`。触发与移动都在**收盘后**完成, 下一根 bar 才生效,
/// 不使用当根 high/low 做前视判定。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakevenConfig {
    /// 触发保本的有利幅度(百分比, 必须位于 `(0, 100)`)。
    pub trigger_pct: f64,
    /// 保本止损相对入场价的偏移(百分比, `>= 0` 且 `< 100`)。
    #[serde(default)]
    pub offset_pct: f64,
}

/// 持仓期风险覆盖(按 `[[strategy]]` 配置)。
///
/// 百分比字段都是 `2.0` = 2%。`None` 表示该规则关闭。多条规则可同时配置,
/// 引擎取**更紧的一侧**(多头取较大止损价, 空头取较小止损价)。止损只作用于
/// **引擎已成交的持仓**, 不改变策略信号本身。
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StopConfig {
    /// 固定百分比止损: 多头 `entry × (1 - pct/100)`, 空头 `entry × (1 + pct/100)`。
    pub stop_loss_pct: Option<f64>,
    /// 百分比追踪止损: 多头 `extreme × (1 - pct/100)`, 空头 `extreme × (1 + pct/100)`;
    /// `extreme` 为入场后的最高 high / 最低 low。
    pub trailing_stop_pct: Option<f64>,
    /// ATR 固定/追踪止损。
    pub atr: Option<AtrStopConfig>,
    /// 保本止损。
    pub breakeven: Option<BreakevenConfig>,
}

impl StopConfig {
    /// 是否未配置任何止损。
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.stop_loss_pct.is_none()
            && self.trailing_stop_pct.is_none()
            && self.atr.is_none()
            && self.breakeven.is_none()
    }

    /// 校验百分比与 ATR/保本参数范围。
    ///
    /// # Errors
    /// 百分比不在 `(0, 100)`、ATR 周期为 0、ATR 倍数非正/非有限、
    /// 保本触发不在 `(0, 100)` 或偏移不在 `[0, 100)` 时返回
    /// [`crate::error::AppError::Msg`]。
    pub fn validate(&self) -> crate::error::Result<()> {
        for (name, value) in [
            ("stop_loss_pct", self.stop_loss_pct),
            ("trailing_stop_pct", self.trailing_stop_pct),
        ] {
            if let Some(p) = value
                && !(p.is_finite() && p > 0.0 && p < 100.0)
            {
                return Err(crate::error::AppError::Msg(format!(
                    "{name} 必须是 (0, 100) 内的有限百分比, 当前为 {p}"
                )));
            }
        }
        if let Some(atr) = self.atr {
            if atr.period == 0 {
                return Err(crate::error::AppError::Msg(
                    "stop.atr.period 必须 >= 1".to_string(),
                ));
            }
            if !(atr.mult.is_finite() && atr.mult > 0.0) {
                return Err(crate::error::AppError::Msg(format!(
                    "stop.atr.mult 必须是 > 0 的有限值, 当前为 {}",
                    atr.mult
                )));
            }
        }
        if let Some(be) = self.breakeven {
            if !(be.trigger_pct.is_finite() && be.trigger_pct > 0.0 && be.trigger_pct < 100.0) {
                return Err(crate::error::AppError::Msg(format!(
                    "stop.breakeven.trigger_pct 必须是 (0, 100) 内的有限百分比, 当前为 {}",
                    be.trigger_pct
                )));
            }
            if !(be.offset_pct.is_finite() && (0.0..100.0).contains(&be.offset_pct)) {
                return Err(crate::error::AppError::Msg(format!(
                    "stop.breakeven.offset_pct 必须是 [0, 100) 内的有限百分比, 当前为 {}",
                    be.offset_pct
                )));
            }
        }
        Ok(())
    }

    /// 参数串(用于报告 `params` 后缀与缓存指纹)。
    #[must_use]
    pub fn param_str(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = self.stop_loss_pct {
            parts.push(format!("stop_loss_pct={v}"));
        }
        if let Some(v) = self.trailing_stop_pct {
            parts.push(format!("trailing_stop_pct={v}"));
        }
        if let Some(atr) = self.atr {
            parts.push(format!("atr_period={}", atr.period));
            parts.push(format!("atr_mult={}", atr.mult));
            if atr.trailing {
                parts.push("atr_trailing=true".to_string());
            }
        }
        if let Some(be) = self.breakeven {
            parts.push(format!("breakeven_trigger_pct={}", be.trigger_pct));
            parts.push(format!("breakeven_offset_pct={}", be.offset_pct));
        }
        parts.join(",")
    }

    /// 把止损/仓位参数追加到策略参数串; 两者都未配置时**原样返回**,
    /// 保证旧结果键不变。
    #[must_use]
    pub fn apply_to(&self, base: &str, sizing: &SizingConfig) -> String {
        let stop = self.param_str();
        let size = sizing.param_str();
        match (stop.is_empty(), size.is_empty()) {
            (true, true) => base.to_string(),
            (false, true) => format!("{base};{stop}"),
            (true, false) => format!("{base};{size}"),
            (false, false) => format!("{base};{stop},{size}"),
        }
    }
}

/// 仓位模式(按 `[[strategy]].sizing` 配置)。
///
/// 只决定**开仓名义**, 不改变信号; `fixed` 腿始终按 `initial_cash` 计算,
/// 复利腿按当前 `cash` 计算。所有模式的名义比例都会截断到
/// `[0, max_position_pct/100]`, 且不超过 1.0 —— 当前引擎没有保证金/杠杆模型。
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SizingConfig {
    /// 全仓复利(默认): `qty = cash / price`。
    #[default]
    AllIn,
    /// 固定仓位: 当前权益的固定百分比。
    Fixed {
        /// 仓位百分比, `20.0` = 20% 权益。
        fraction_pct: f64,
    },
    /// Kelly 仓位: 用最近 `lookback` 笔已平仓交易的 `pnl_pct` 估计胜率/赔率。
    Kelly {
        /// 回看交易笔数。
        lookback: usize,
        /// 最少已决交易笔数; 不足时用 `fallback_fraction_pct`。
        min_trades: usize,
        /// Kelly 折减系数, `0.5` = 半 Kelly。
        kelly_fraction: f64,
        /// 单品种最大名义占权益百分比。
        #[serde(default = "default_max_position_pct")]
        max_position_pct: f64,
        /// 样本不足时的固定仓位百分比; 必须 > 0, 否则永远无法积累样本。
        #[serde(default = "default_kelly_fallback_pct")]
        fallback_fraction_pct: f64,
    },
    /// ATR 风险仓位: `f = risk_pct/100 × price / (mult × ATR)`。
    Atr {
        /// 单笔风险占权益百分比。
        risk_pct: f64,
        /// ATR 周期。
        period: usize,
        /// 风险单位 = `mult × ATR`。
        mult: f64,
        /// 单品种最大名义占权益百分比。
        #[serde(default = "default_max_position_pct")]
        max_position_pct: f64,
        /// ATR 未就绪时的固定仓位百分比; `0` = 不进场。
        #[serde(default)]
        fallback_fraction_pct: f64,
    },
}

fn default_max_position_pct() -> f64 {
    100.0
}

fn default_kelly_fallback_pct() -> f64 {
    10.0
}

impl SizingConfig {
    /// 是否为默认全仓模式。
    #[must_use]
    pub fn is_all_in(&self) -> bool {
        matches!(self, Self::AllIn)
    }

    /// 该模式是否需要 high/low 计算 ATR。
    #[must_use]
    pub fn needs_ohlc(&self) -> bool {
        matches!(self, Self::Atr { .. })
    }

    /// 校验参数范围。
    ///
    /// # Errors
    /// 百分比越界、周期为 0、Kelly 样本门槛矛盾时返回
    /// [`crate::error::AppError::Msg`]。
    pub fn validate(&self) -> crate::error::Result<()> {
        match *self {
            Self::AllIn => Ok(()),
            Self::Fixed { fraction_pct } => {
                if !(fraction_pct.is_finite() && fraction_pct > 0.0 && fraction_pct <= 100.0) {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.fixed.fraction_pct 必须是 (0, 100] 内的有限百分比, 当前为 {fraction_pct}"
                    )));
                }
                Ok(())
            }
            Self::Kelly {
                lookback,
                min_trades,
                kelly_fraction,
                max_position_pct,
                fallback_fraction_pct,
            } => {
                if lookback == 0 {
                    return Err(crate::error::AppError::Msg(
                        "sizing.kelly.lookback 必须 >= 1".to_string(),
                    ));
                }
                if min_trades < 2 || min_trades > lookback {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.kelly.min_trades 必须位于 [2, lookback={lookback}], 当前为 {min_trades}"
                    )));
                }
                if !(kelly_fraction.is_finite() && kelly_fraction > 0.0 && kelly_fraction <= 1.0) {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.kelly.kelly_fraction 必须是 (0, 1] 内的有限值, 当前为 {kelly_fraction}"
                    )));
                }
                if !(max_position_pct.is_finite()
                    && max_position_pct > 0.0
                    && max_position_pct <= 100.0)
                {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.kelly.max_position_pct 必须是 (0, 100] 内的有限百分比, 当前为 {max_position_pct}"
                    )));
                }
                if !(fallback_fraction_pct.is_finite()
                    && fallback_fraction_pct > 0.0
                    && fallback_fraction_pct <= 100.0)
                {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.kelly.fallback_fraction_pct 必须是 (0, 100] 内的有限百分比, 当前为 {fallback_fraction_pct}"
                    )));
                }
                Ok(())
            }
            Self::Atr {
                risk_pct,
                period,
                mult,
                max_position_pct,
                fallback_fraction_pct,
            } => {
                if !(risk_pct.is_finite() && risk_pct > 0.0 && risk_pct <= 100.0) {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.atr.risk_pct 必须是 (0, 100] 内的有限百分比, 当前为 {risk_pct}"
                    )));
                }
                if period == 0 {
                    return Err(crate::error::AppError::Msg(
                        "sizing.atr.period 必须 >= 1".to_string(),
                    ));
                }
                if !(mult.is_finite() && mult > 0.0) {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.atr.mult 必须是 > 0 的有限值, 当前为 {mult}"
                    )));
                }
                if !(max_position_pct.is_finite()
                    && max_position_pct > 0.0
                    && max_position_pct <= 100.0)
                {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.atr.max_position_pct 必须是 (0, 100] 内的有限百分比, 当前为 {max_position_pct}"
                    )));
                }
                if !(fallback_fraction_pct.is_finite()
                    && (0.0..=100.0).contains(&fallback_fraction_pct))
                {
                    return Err(crate::error::AppError::Msg(format!(
                        "sizing.atr.fallback_fraction_pct 必须是 [0, 100] 内的有限百分比, 当前为 {fallback_fraction_pct}"
                    )));
                }
                Ok(())
            }
        }
    }

    /// 参数串(用于报告 `params` 后缀与缓存指纹)。
    ///
    /// `all_in` 返回空串: 与旧配置/旧报告键完全一致, 不制造无意义的缓存失效。
    #[must_use]
    pub fn param_str(&self) -> String {
        match *self {
            Self::AllIn => String::new(),
            Self::Fixed { fraction_pct } => {
                format!("size=fixed,size_fraction_pct={fraction_pct}")
            }
            Self::Kelly {
                lookback,
                min_trades,
                kelly_fraction,
                max_position_pct,
                fallback_fraction_pct,
            } => format!(
                "size=kelly,size_lookback={lookback},size_min_trades={min_trades},\
                 size_kelly_fraction={kelly_fraction},size_max_position_pct={max_position_pct},\
                 size_fallback_fraction_pct={fallback_fraction_pct}"
            ),
            Self::Atr {
                risk_pct,
                period,
                mult,
                max_position_pct,
                fallback_fraction_pct,
            } => format!(
                "size=atr,size_risk_pct={risk_pct},size_period={period},size_mult={mult},\
                 size_max_position_pct={max_position_pct},\
                 size_fallback_fraction_pct={fallback_fraction_pct}"
            ),
        }
    }
}

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
    /// ([`simulate`] / [`simulate_detailed`]) 都会校验。
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
    fn validate_rates(&self) -> crate::error::Result<()> {
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

#[derive(Debug, Clone)]
/// 一笔已平仓交易。仅 `--detail` 路径产出(非详细路径只累计标量统计)。
pub struct Trade {
    /// 开仓 bar 的时间戳(ms)。
    pub entry_time: i64,
    /// 平仓 bar 的时间戳(ms)。
    pub exit_time: i64,
    /// 持仓方向: `Long` / `Short`。恒不为 `Flat`。
    pub side: Side,
    /// 开仓成交价(含逆向滑点)。
    pub entry_price: f64,
    /// 平仓成交价(含逆向滑点)。
    pub exit_price: f64,
    /// 复利腿持仓数量(金额量纲, 定点十进制)。
    pub qty: crate::money::Amount,
    /// 复利腿盈亏(货币, 已扣双边佣金; 定点十进制)。
    pub pnl: crate::money::Amount,
    /// 相对开仓价的收益率(小数)。
    pub pnl_pct: f64,
    /// fixed 腿盈亏(固定名义, 已扣双边佣金; 定点十进制)。
    pub fixed_pnl: crate::money::Amount,
    /// 持仓 bar 数。
    pub bars_held: i64,
    /// 持仓期内最大有利偏移(价格差)。
    pub max_runup: f64,
    /// 持仓期内最大不利偏移(价格差)。
    pub max_drawdown: f64,
    /// 平仓原因。取值集合是**封闭**的(见 [`ExitReason`]), 故用 `&'static str`
    /// 而非 `String`: 原实现每个平仓都 `format!`-出一个堆 `String`, 而参数网格
    /// 动辄几十万笔, 这是一次纯浪费的分配 + 拷贝。
    pub exit_reason: &'static str,
}

/// 平仓原因的封闭集合。
///
/// 枚举化是为了让 `exit_reason` 的取值**在编译期封闭** —— 落盘层的字典序统计
/// 不再需要防御未知字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// 持仓被反向信号平掉。
    Signal,
    /// 固定百分比/ATR 止损触发。
    StopLoss,
    /// 百分比/ATR 追踪止损或保本止损触发。
    TrailingStop,
    /// 策略提供的止损价格线触发。
    StrategyStop,
    /// 数据末尾强制平仓。
    EndOfData,
}

impl ExitReason {
    /// 落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::StopLoss => "stop_loss",
            Self::TrailingStop => "trailing_stop",
            Self::StrategyStop => "strategy_stop",
            Self::EndOfData => "end_of_data",
        }
    }
}

#[derive(Debug, Clone)]
/// `--detail` 路径的一次求值结果。
pub struct SimResult {
    /// 全部指标(与 [`simulate`] 逐位一致)。
    pub summary: SummaryMetrics,
    /// 逐笔已平仓交易。
    pub trades: Vec<Trade>,
    /// 复利腿逐 bar `(时间戳, 权益)`。
    pub equity_curve: Vec<(f64, f64)>,
    /// fixed 腿逐 bar 权益(与 bar 对齐, `--pbo` 的 T×N 输入)。
    pub fixed_curve: Vec<f64>,
}

/// 信号 → **目标持仓方向** 的单步状态机。
///
/// 为什么不是一个数组: 原实现先物化一份 O(T) 的 `Vec<Side>`, 而 `scan_trades`
/// 只在**当前 bar** 用到 `positions[i]`。物化版本的代价是每次参数求值多分配
/// 一条与 bar 数等长的向量(T=1M 时 1 MB/档), 参数网格动辄 10 万档 ⇒ 百 GB 级
/// 的额外分配流量, 与本仓库"内存是第一约束"的设计前提直接冲突。
///
/// 状态机是**前缀累积**的, 故逐 bar 增量维护与先物化再索引**逐位等价**。
#[inline]
fn next_target(prev: Side, sig: Signal) -> Side {
    match sig {
        Signal::Buy => Side::Long,
        Signal::Sell => Side::Short,
        Signal::Close => Side::Flat,
        Signal::ExitLong if prev == Side::Long => Side::Flat,
        Signal::ExitShort if prev == Side::Short => Side::Flat,
        Signal::ExitLong | Signal::ExitShort | Signal::Hold => prev,
    }
}

#[derive(Debug, Clone, Copy)]
struct CloseArgs {
    cur_pos: Side,
    /// 持仓数量(金额量纲, 定点十进制)。
    pos_qty: crate::money::Amount,
    pos_entry: f64,
    pos_entry_time: i64,
    pos_entry_bar: usize,
    exit_price: f64,
    exit_time: i64,
    exit_bar: usize,
    /// 本笔 fixed 腿的持仓数量(定点十进制)。开仓时按仓位模式确定,
    /// 平仓时必须复用同一数量, 不能按 initial_cash 重算。
    fixed_qty: crate::money::Amount,
    trade_mfe: f64,
    trade_mae: f64,
    fee_rate: f64,
    /// 平仓原因(信号反手 / 数据末尾强制)。
    reason: ExitReason,
}

/// 方向 side 的成交价: 买入抬价、卖出压价(逆向滑点)。
fn fill_price(side: Side, base: f64, slip_rate: f64) -> f64 {
    if side == Side::Long {
        base * (1.0 + slip_rate)
    } else {
        base * (1.0 - slip_rate)
    }
}

/// 被动基准: 首根 close 买入、末根 close 卖出, 无成本, 固定名义。
/// 返回 (pnl, return)。数据不足 2 根或首价非正有限值时返回 (0, 0)。
/// 与策略 fixed 腿同为固定名义口径, 可直接对比。
///
/// `pnl` 是**金额**(定点十进制), `return` 是**比率**(无量纲 ⇒ `f64`)。
pub fn buy_hold_pnl(
    close: &[f64],
    nominal: crate::money::Amount,
) -> crate::error::Result<(crate::money::Amount, f64)> {
    if close.len() < 2 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let (first, last) = (close[0], close[close.len() - 1]);
    if !first.is_finite() || !last.is_finite() || first <= 0.0 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let ret = last / first - 1.0;
    // 金额 = 名义 × 收益率; 名义是金额, 收益率是无量纲比率。
    let pnl = nominal.mul_ratio(ret)?;
    Ok((pnl, ret))
}

/// 成交价是否**可用**: 有限且为正。
///
/// 成交价是整条成交链的除数: `qty = cash / price`、`pnl_pct = (exit - entry) / entry`。
/// 价格为 0 ⇒ `qty = inf` ⇒ 权益变 NaN ⇒ **全部指标被污染**; 且 NaN 比 0 更危险 ——
/// 0 会被读成"零波动低风险", NaN 才会暴露。价格非有限(NaN/Inf)同理。
///
/// 数据层的非法价格在**加载边界**被拒(见 `app::validate_columns`); 此处是引擎
/// 侧的兜底, 保证引擎被单独调用(库用户)时也不会产出 NaN 指标。
#[inline]
fn valid_fill_price(p: f64) -> bool {
    p.is_finite() && p > 0.0
}

/// fill bar `bar`、订单方向 `side` 的成交价。
///
/// `fill_close=false`(默认)取开盘, `true` 取收盘; 开盘缺失时退回收盘。
/// 成交价非有限或非正时返回 `None` —— 调用方据此**放弃本 bar 的成交**,
/// 持仓状态保持不变, 而不是拿会污染全部指标的 NaN/0 价格去结算。
///
/// 成交只发生在信号 bar + `delay_bars` 的 fill bar 上(`delay_bars` 恒 ≥ 1,
/// 由 `NonZeroUsize` 保证), 故调用方必须先排队、到期后再调用本函数。
#[inline]
fn fill_at(bar: usize, side: Side, close: &[f64], open: &[f64], exec: &ExecConfig) -> Option<f64> {
    let base = if exec.fill_close {
        close[bar]
    } else {
        match open.get(bar).copied() {
            Some(p) => p,
            None => {
                // 生产路径不会走到这里: `KlineColumns` 强制各列等长。只有直接
                // 调公共 API 且 `open` 偏短时才会触发 —— 退回收盘价并留一条
                // **一次性** debug 记录, 避免静默改变成交价假设。
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| {
                    tracing::debug!(bar, "open 缺失, 成交价回退到 close(仅提示一次)");
                });
                close[bar]
            }
        }
    };
    let price = fill_price(side, base, exec.slip_rate);
    valid_fill_price(price).then_some(price)
}

/// 一笔平仓的**标量**结果。
///
/// 与 [`Trade`] 分离的原因: `simulate()`(非 detailed 路径)只需要标量来算统计量,
/// 根本不需要 `Trade`。原先两者被绑在一起, 导致每个参数求值都要为每笔交易
/// 构造一个 `Trade` —— 其中 `exit_reason: String` 是**堆分配**。参数网格是串行
/// 外循环(策略层并行会 OOM, 见 README), 这笔开销直接乘以总交易笔数。
#[derive(Debug, Clone, Copy)]
struct CloseOutcome {
    /// 现金变动(定点十进制)。
    cash_delta: crate::money::Amount,
    /// 本笔盈亏(定点十进制)。
    pnl: crate::money::Amount,
    /// 本笔收益率(**比率**, 无量纲 ⇒ 仍是 `f64`)。
    pnl_pct: f64,
    /// fixed 腿本笔盈亏(定点十进制)。
    fixed_pnl: crate::money::Amount,
    bars_held: i64,
    max_runup: f64,
    max_drawdown: f64,
    /// 平仓原因, 用于累计止损笔数。
    reason: ExitReason,
}

/// 已平仓交易的累计量。非 detailed 路径只需要它, 不需要保留逐笔 [`Trade`]。
#[derive(Debug, Default)]
struct TradeAccum {
    num_trades: u32,
    /// 固定百分比/ATR 止损离场笔数。
    stop_loss_exits: u32,
    /// 百分比/ATR 追踪或保本止损离场笔数。
    trailing_stop_exits: u32,
    /// 策略止损线离场笔数。
    strategy_stop_exits: u32,
    /// 逐笔 `pnl`: SQN / skew / kurtosis / pf-stability 的分半统计都要用它。
    ///
    /// **统计序列用 `f64`**: 这些是**统计量**(偏度/峰度/SQN), 本身是连续量,
    /// 不是"账上的钱"。账上的钱(累计 `total_fixed_pnl`)是 `Amount`。
    pnls: Vec<f64>,
    /// 逐笔 `fixed_pnl`: fixed 腿的 win/loss 拆分与 expectancy(统计序列)。
    fixed_pnls: Vec<f64>,
    /// 逐笔 `pnl_pct`(毛收益率, 与仓位大小无关): Kelly 仓位的估计样本。
    pnl_pcts: Vec<f64>,
    /// 累计 fixed 腿盈亏(**金额**, 定点十进制)。
    total_fixed_pnl: crate::money::Amount,
    /// 盈利笔累计盈亏(**金额**, 定点十进制)。
    total_win: crate::money::Amount,
    /// 亏损笔累计亏损绝对值(**金额**, 定点十进制)。
    total_loss: crate::money::Amount,
    /// 逐笔 `pnl_pct` 之和(仅用于 avg pnl_pct; 无需保留整个序列)。
    pnl_pct_sum: f64,
    /// MFE / MAE / 持仓 bar 数之和。**报告列 `avg_mfe`/`avg_mae`/`avg_hold_bars`
    /// 需要它们**, 故非 detailed 路径也必须累计(只需三个标量, 不必留逐笔 Trade)。
    sum_mfe: f64,
    sum_mae: f64,
    sum_hold: f64,
}

/// 计算一次平仓的**钱**与收益率。
///
/// 钱的部分全部走 [`Amount`](crate::money::Amount)(定点十进制): 佣金、
/// 现金变动、单笔盈亏、fixed 腿盈亏。价格与费率仍是 `f64`(价格是连续量,
/// 费率无量纲), 但**钱**本身不再用 `f64` 承载。
///
/// # Errors
/// 金额运算溢出、或除以非法价格时返回 `AppError`(crate::error::AppError)。
fn close_outcome(args: CloseArgs) -> crate::error::Result<CloseOutcome> {
    let CloseArgs {
        cur_pos,
        pos_qty,
        pos_entry,
        pos_entry_bar,
        exit_price,
        exit_bar,
        fixed_qty,
        trade_mfe,
        trade_mae,
        fee_rate,
        ..
    } = args;

    // 平仓单方向与持仓反向: 多头平仓为卖, 空头平仓为买; 佣金按单边名义扣除。
    // 开仓佣金在建仓时已从 `cash` 扣掉, 但**逐笔 `pnl` 必须同样扣掉它** ——
    // 否则 `sum(trade.pnl) != final_equity - initial_cash`, win/loss/PF/SQN
    // 等一切基于逐笔 pnl 的统计都会系统性偏乐观。
    let entry_fee = pos_qty.mul_price(pos_entry)?.mul_ratio(fee_rate)?;
    let exit_fee = pos_qty.mul_price(exit_price)?.mul_ratio(fee_rate)?;
    let (cash_delta, pnl) = if cur_pos == Side::Long {
        (
            pos_qty.mul_price(exit_price)?.minus(exit_fee)?,
            pos_qty
                .mul_price(exit_price - pos_entry)?
                .minus(entry_fee)?
                .minus(exit_fee)?,
        )
    } else {
        (
            pos_qty.mul_price(exit_price)?.negated().minus(exit_fee)?,
            pos_qty
                .mul_price(pos_entry - exit_price)?
                .minus(entry_fee)?
                .minus(exit_fee)?,
        )
    };
    let entry_fee_fixed = fixed_qty.mul_price(pos_entry)?.mul_ratio(fee_rate)?;
    let exit_fee_fixed = fixed_qty.mul_price(exit_price)?.mul_ratio(fee_rate)?;
    let fixed_pnl = if cur_pos == Side::Long {
        fixed_qty
            .mul_price(exit_price - pos_entry)?
            .minus(entry_fee_fixed)?
            .minus(exit_fee_fixed)?
    } else {
        fixed_qty
            .mul_price(pos_entry - exit_price)?
            .minus(entry_fee_fixed)?
            .minus(exit_fee_fixed)?
    };
    // 收益率是**比率**(无量纲)⇒ 仍是 `f64`。
    //
    // **有意与 `pnl` 不同口径**: `pnl`/`fixed_pnl` 是**净额**(已扣双边佣金),
    // 而 `pnl_pct` 是**毛比率**(成交价已含滑点, 不再扣佣金)。二者并存是为了
    // 让 `expectancy` 保持"策略边际"语义、与费率配置解耦; 需要净期望时用
    // `SummaryMetrics::fixed_expectancy_bps`。
    let pnl_pct = if cur_pos == Side::Long {
        (exit_price - pos_entry) / pos_entry
    } else {
        (pos_entry - exit_price) / pos_entry
    };
    Ok(CloseOutcome {
        cash_delta,
        pnl,
        pnl_pct,
        fixed_pnl,
        bars_held: (exit_bar as i64) - (pos_entry_bar as i64),
        max_runup: trade_mfe,
        max_drawdown: trade_mae,
        reason: args.reason,
    })
}

/// 由标量结果物化完整 [`Trade`]。**仅 detailed 路径调用**。
fn materialize_trade(args: &CloseArgs, out: CloseOutcome) -> Trade {
    Trade {
        entry_time: args.pos_entry_time,
        exit_time: args.exit_time,
        side: args.cur_pos,
        entry_price: args.pos_entry,
        exit_price: args.exit_price,
        qty: args.pos_qty,
        pnl: out.pnl,
        pnl_pct: out.pnl_pct,
        fixed_pnl: out.fixed_pnl,
        bars_held: out.bars_held,
        max_runup: out.max_runup,
        max_drawdown: out.max_drawdown,
        exit_reason: args.reason.as_str(),
    }
}

/// 平仓时需要更新的账本/产物引用。
///
/// 单独成结构体而不是把 6 个 `&mut` 参数平铺: 调用点只构造一次, 且
/// 「信号平仓 / 止损平仓 / 末根强平」三条路径共用同一段资金结算逻辑。
struct CloseState<'a> {
    cash: &'a mut crate::money::Amount,
    acc: &'a mut TradeAccum,
    trades: &'a mut Vec<Trade>,
    fixed_realized: &'a mut crate::money::Amount,
    fixed_qty: &'a mut crate::money::Amount,
    collect_curves: bool,
}

/// 结算一笔平仓并更新所有账本/产物。
fn close_at(args: CloseArgs, st: &mut CloseState<'_>) -> crate::error::Result<()> {
    let out = close_outcome(args)?;
    *st.cash = st.cash.plus(out.cash_delta)?;
    st.acc.push(out)?;
    if st.collect_curves {
        st.trades.push(materialize_trade(&args, out));
    }
    *st.fixed_realized = st.fixed_realized.plus(out.fixed_pnl)?;
    *st.fixed_qty = crate::money::Amount::ZERO;
    Ok(())
}

/// 两个止损价中更紧的一条: 多头取较高价, 空头取较低价。
///
/// 非法价格(非有限/`<= 0`)视为未设置。两边都非法时返回 `None`。
fn tighter_stop(side: Side, a: Option<f64>, b: Option<f64>) -> Option<f64> {
    let a = a.filter(|p| valid_fill_price(*p));
    let b = b.filter(|p| valid_fill_price(*p));
    match (side, a, b) {
        (Side::Long, Some(x), Some(y)) => Some(x.max(y)),
        (Side::Short, Some(x), Some(y)) => Some(x.min(y)),
        (Side::Long | Side::Short, Some(x), None) | (Side::Long | Side::Short, None, Some(x)) => {
            Some(x)
        }
        (Side::Flat, _, _) | (_, None, None) => None,
    }
}

/// Wilder 平滑 ATR。前 `period` 根没有可用值, 以 `NaN` 填充。
///
/// 引擎只用已收盘 bar 的 ATR: 开盘成交取 `atr[bar - 1]`, 收盘成交取
/// `atr[bar]`; 追踪止损在收盘后更新, 下一根 bar 生效。
fn wilder_atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut atr = vec![f64::NAN; n];
    if period == 0 || n <= period {
        return atr;
    }
    let tr = |i: usize| -> f64 {
        let h = high.get(i).copied().unwrap_or(close[i]);
        let l = low.get(i).copied().unwrap_or(close[i]);
        let prev = close[i - 1];
        if !h.is_finite() || !l.is_finite() || !prev.is_finite() {
            return f64::NAN;
        }
        (h - l).max((h - prev).abs()).max((l - prev).abs())
    };
    let mut sum = 0.0;
    for i in 1..=period {
        sum += tr(i);
    }
    atr[period] = sum / period as f64;
    for i in (period + 1)..n {
        atr[i] = (atr[i - 1] * (period - 1) as f64 + tr(i)) / period as f64;
    }
    atr
}

/// 入场时使用的 ATR: 开盘成交取上一根已收盘 bar, 收盘成交取当前 bar。
fn atr_at_entry(atr: &[f64], bar: usize, fill_close: bool) -> Option<f64> {
    let idx = if fill_close { bar } else { bar.checked_sub(1)? };
    atr.get(idx).copied().filter(|v| v.is_finite() && *v > 0.0)
}

/// 把名义比例截断到 `[0, max_pct/100]`, 且不超过 1.0(当前引擎无杠杆)。
fn clamp_fraction(f: f64, max_pct: f64) -> Option<f64> {
    if !f.is_finite() || f <= 0.0 {
        return None;
    }
    Some(f.min(max_pct / 100.0).min(1.0))
}

/// 用最近 `lookback` 笔已平仓交易的毛收益率估计 Kelly 比例。
///
/// 采用二元胜负近似: `p = 胜率`, `b = 平均盈利 / 平均亏损`,
/// `f = p - (1-p)/b`。样本不足或只有单边结果时返回 `None`(走 fallback);
/// 估计出的 `f <= 0` 返回 `Some(0.0)`(有样本但无正期望, 不开仓)。
fn kelly_fraction_from_trades(
    returns: &[f64],
    lookback: usize,
    min_trades: usize,
    kelly_fraction: f64,
) -> Option<f64> {
    let start = returns.len().saturating_sub(lookback);
    let recent = &returns[start..];
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut sum_win = 0.0;
    let mut sum_loss = 0.0;
    for &r in recent {
        if !r.is_finite() {
            continue;
        }
        if r > 0.0 {
            wins += 1;
            sum_win += r;
        } else if r < 0.0 {
            losses += 1;
            sum_loss -= r;
        }
    }
    let decided = wins + losses;
    if decided < min_trades || wins == 0 || losses == 0 {
        return None;
    }
    let p = wins as f64 / decided as f64;
    let avg_win = sum_win / wins as f64;
    let avg_loss = sum_loss / losses as f64;
    if !avg_loss.is_finite() || avg_loss <= 0.0 {
        return None;
    }
    let b = avg_win / avg_loss;
    let f = p - (1.0 - p) / b;
    if f.is_finite() && f > 0.0 {
        Some(f * kelly_fraction)
    } else {
        Some(0.0)
    }
}

/// 按仓位模式计算本笔开仓占当前权益的比例。
fn entry_fraction(
    sizing: &SizingConfig,
    acc: &TradeAccum,
    sizing_atr: Option<&[f64]>,
    bar: usize,
    fill_close: bool,
    entry_price: f64,
) -> Option<f64> {
    match *sizing {
        SizingConfig::AllIn => Some(1.0),
        SizingConfig::Fixed { fraction_pct } => clamp_fraction(fraction_pct / 100.0, 100.0),
        SizingConfig::Kelly {
            lookback,
            min_trades,
            kelly_fraction,
            max_position_pct,
            fallback_fraction_pct,
        } => {
            let f = kelly_fraction_from_trades(&acc.pnl_pcts, lookback, min_trades, kelly_fraction)
                .unwrap_or(fallback_fraction_pct / 100.0);
            clamp_fraction(f, max_position_pct)
        }
        SizingConfig::Atr {
            risk_pct,
            period: _,
            mult,
            max_position_pct,
            fallback_fraction_pct,
        } => {
            let atr = sizing_atr
                .and_then(|a| atr_at_entry(a, bar, fill_close))
                .filter(|v| v.is_finite() && *v > 0.0);
            let f = atr
                .map(|atr| (risk_pct / 100.0) * entry_price / (mult * atr))
                .unwrap_or(fallback_fraction_pct / 100.0);
            clamp_fraction(f, max_position_pct)
        }
    }
}

/// 开仓数量: 复利腿按当前 `cash`, fixed 腿按 `initial_cash`。
///
/// 返回 `None` 表示本 bar 不满足开仓条件(ATR 未就绪 / Kelly 无正期望 /
/// 比例过小), 调用方应保持空仓并在下一 bar 重试。
#[allow(clippy::too_many_arguments)]
fn entry_qty(
    sizing: &SizingConfig,
    acc: &TradeAccum,
    sizing_atr: Option<&[f64]>,
    bar: usize,
    fill_close: bool,
    entry_price: f64,
    cash: crate::money::Amount,
    initial_cash: crate::money::Amount,
) -> crate::error::Result<Option<(crate::money::Amount, crate::money::Amount)>> {
    let Some(f) = entry_fraction(sizing, acc, sizing_atr, bar, fill_close, entry_price) else {
        return Ok(None);
    };
    // AllIn 走原路径, 保证与旧结果逐位一致。
    if sizing.is_all_in() {
        let qty = cash.div_price(entry_price)?;
        let fixed_qty = initial_cash.div_price(entry_price)?;
        return Ok(
            (qty > crate::money::Amount::ZERO && fixed_qty > crate::money::Amount::ZERO)
                .then_some((qty, fixed_qty)),
        );
    }
    let qty = cash.mul_ratio(f)?.div_price(entry_price)?;
    let fixed_qty = initial_cash.mul_ratio(f)?.div_price(entry_price)?;
    Ok(
        (qty > crate::money::Amount::ZERO && fixed_qty > crate::money::Amount::ZERO)
            .then_some((qty, fixed_qty)),
    )
}

/// 当前持仓的有效止损价与原因。
///
/// 同时配置多条规则时取**更紧**的一条: 多头取较高价, 空头取较低价。
/// 价格相同时按固定止损 > 策略止损 > 追踪/保本的顺序归因。
fn active_stop(
    cur_pos: Side,
    stop_loss: Option<f64>,
    strategy_stop: Option<f64>,
    trailing_stop: Option<f64>,
    breakeven_stop: Option<f64>,
) -> Option<(f64, ExitReason)> {
    let mut best: Option<(f64, ExitReason)> = None;
    for (value, reason) in [
        (stop_loss, ExitReason::StopLoss),
        (strategy_stop, ExitReason::StrategyStop),
        (trailing_stop, ExitReason::TrailingStop),
        (breakeven_stop, ExitReason::TrailingStop),
    ] {
        let Some(value) = value.filter(|p| valid_fill_price(*p)) else {
            continue;
        };
        let stricter = match best {
            None => true,
            Some((current, _)) if cur_pos == Side::Long => value > current,
            Some((current, _)) => value < current,
        };
        if stricter {
            best = Some((value, reason));
        }
    }
    best
}

/// 本 bar 是否触发止损, 返回**未扣滑点**的成交基准价与原因。
///
/// 盘中触发: 多头 `low <= stop`, 空头 `high >= stop`。跳空时按 `open` 成交
/// (多头向下跳空、空头向上跳空), 否则按止损价成交。
#[allow(clippy::too_many_arguments)]
fn stop_exit_price(
    bar: usize,
    cur_pos: Side,
    high: &[f64],
    low: &[f64],
    open: &[f64],
    close: &[f64],
    stop_loss: Option<f64>,
    strategy_stop: Option<f64>,
    trailing_stop: Option<f64>,
    breakeven_stop: Option<f64>,
) -> Option<(f64, ExitReason)> {
    let (stop, reason) = active_stop(
        cur_pos,
        stop_loss,
        strategy_stop,
        trailing_stop,
        breakeven_stop,
    )?;
    if !stop.is_finite() || stop <= 0.0 {
        return None;
    }
    let fallback = close.get(bar).copied()?;
    let o = open
        .get(bar)
        .copied()
        .filter(|p| p.is_finite() && *p > 0.0)
        .unwrap_or(fallback);
    let h = high.get(bar).copied().unwrap_or(fallback);
    let l = low.get(bar).copied().unwrap_or(fallback);
    if !o.is_finite() || !h.is_finite() || !l.is_finite() {
        return None;
    }
    match cur_pos {
        Side::Long if o <= stop => Some((o, reason)),
        Side::Long if l <= stop => Some((stop, reason)),
        Side::Short if o >= stop => Some((o, reason)),
        Side::Short if h >= stop => Some((stop, reason)),
        _ => None,
    }
}

/// 指标计算的**唯一**数据源。
///
/// 两条路径(详细 / 非详细)**都**填充它, 且填充方式逐行相同 —— 这是
/// "报告数字不随 `--detail` 开关变化"的结构性保证: [`compute_metrics`] 的签名
/// 收 `&MetricsSource`, 因此它在**类型上就看不到** [`DetailArtifacts`]。
///
/// 拆分自原先的单一 `ScanOut`: 那里 6 个字段并列, "指标只读其中 3 个"仅仅是
/// 约定(谁新增一个字段并顺手读它, 就会让两条路径漂移), 而非编译期约束。
#[derive(Debug)]
struct MetricsSource {
    acc: TradeAccum,
    eq: CurveAccum,
    fx: CurveAccum,
    /// 末根权益的**精确定点值**(不是从 f64 曲线回造的近似值)。
    ///
    /// 报告里的 `final_equity` 与 `total_return` 必须由账本直接给出: 先转成
    /// `f64` 再 `Amount::from_f64` 会把 15 位之后的精度丢掉, 与逐笔
    /// `sum(pnl)` 对不上(实测差 1.75e-7)。
    final_equity: crate::money::Amount,
}

/// 仅详细路径需要落盘的产物。
///
/// 为空表示 `collect_curves == false`([`simulate`]); [`simulate_detailed`] 传
/// true 时填充。这些字段**不参与**任何指标计算 —— 指标只读 [`MetricsSource`]。
#[derive(Debug, Default)]
struct DetailArtifacts {
    trades: Vec<Trade>,
    equity: Vec<f64>,
    fixed: Vec<f64>,
}

/// `scan_trades` 的产出: 指标源 + (可选的)落盘产物。
#[derive(Debug)]
struct ScanOut {
    /// 两条路径都有, 且填充方式完全一致。
    metrics: MetricsSource,
    /// 仅 `collect_curves == true` 时有内容; 否则三个字段全空。
    detail: DetailArtifacts,
    /// 是否物化了落盘产物。**不**用 `Option` 表达, 免得调用方为了取产物
    /// 而不得不写一个 `expect()`(那正是 `err-no-unwrap-prod` 反对的东西)。
    collect_curves: bool,
}

/// 扫描信号并模拟成交。
///
/// 时序不变量: 信号 bar `i` 只把目标方向**入队**, 直到 fill bar `i + delay`
/// 才改变 `cur_pos`/`cash`。因此 bar `i` 的权益只由 bar `i` 及之前已发生的
/// 成交决定, 不会按未来成交价盯市(见 `equity_curve_does_not_mark_to_future_fill_price`)。
///
/// `collect_curves`: 是否物化两条权益曲线与逐笔 [`Trade`]。false 时曲线由
/// [`CurveAccum`] 在线累计(O(1)); 但 [`TradeAccum`] 仍保留逐笔 pnl 序列
/// (O(交易笔数), 最坏 O(T))用于 SQN/偏度/峰度/分半 PF —— 不保留 T×16B 的两条曲线。
///
/// `open` 允许比 `close` 短(公共 API 的宽松契约): 缺失的开盘价由 [`fill_at`]
/// 退回收盘价, **不会**因此少回测末尾的 bar。旧实现把 `open.len()` 也算进
/// `n` 的 `min`, 于是 `open = []` 时整段信号被静默丢弃、一笔都不成交。
///
/// `high` / `low` 只在配置了 [`StopConfig`] 或策略提供了 [`StopLines`] 时
/// 参与盘中止损判定; 两者都为空时完全不影响结果, 与旧路径逐位一致。
#[allow(clippy::too_many_arguments)]
fn scan_trades(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    time: &[i64],
    initial_cash: crate::money::Amount,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
    collect_curves: bool,
) -> crate::error::Result<ScanOut> {
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let atr_values = risk.atr.map(|cfg| wilder_atr(high, low, close, cfg.period));
    let sizing_atr = match *sizing {
        SizingConfig::Atr { period, .. } => Some(wilder_atr(high, low, close, period)),
        _ => None,
    };
    let ts = |i: usize| if i < time.len() { time[i] } else { i as i64 };
    // 钱账本: 全部定点十进制(`domain-fintech`: 不用 f64 承载货币)。
    let mut cash = initial_cash;
    let mut pos_qty = crate::money::Amount::ZERO;
    let mut pos_entry = 0.0;
    let mut pos_entry_time: i64 = 0;
    let mut pos_entry_bar: usize = 0;
    let mut cur_pos = Side::Flat;
    // 目标持仓方向: 由信号序列前缀累积而来(见 [`next_target`])。
    // 增量维护, 不物化 O(T) 的 `Vec<Side>`。
    let mut target = Side::Flat;
    let mut trades: Vec<Trade> = if collect_curves {
        Vec::with_capacity(1024)
    } else {
        Vec::new()
    };
    let mut acc = TradeAccum {
        pnls: Vec::with_capacity(1024),
        fixed_pnls: Vec::with_capacity(1024),
        pnl_pcts: Vec::with_capacity(1024),
        ..TradeAccum::default()
    };
    let mut eq_acc = CurveAccum::new(initial_cash.to_f64());
    let mut fx_acc = CurveAccum::new(initial_cash.to_f64());
    // 曲线容器: 不收集时保持为空(Vec::new() 不分配), 收集时一次预分配到位。
    // 刻意**不用** `Option<Vec<f64>>`: 那会让调用方为了取产物而写 `expect()`。
    let mut equity_curve: Vec<f64> = Vec::new();
    // fixed 腿: 每笔按 initial/entry 固定张数, 逐 bar 盯市, 跨笔不复利。
    let mut fixed_curve: Vec<f64> = Vec::new();
    if collect_curves {
        equity_curve.reserve(n);
        fixed_curve.reserve(n);
    }
    // 末根权益暂存: 强制平仓会把它从"盯市值"改写为"已实现 cash", 而在线累加器
    // 无法撤回已消费的点, 故末根**推迟**到平仓之后再入累加器/曲线。
    // 同时保留**精确定点值**: 报告的 `final_equity` 不能从 f64 曲线回造。
    let mut tail_eq = initial_cash.to_f64();
    let mut tail_eq_amt = initial_cash;
    let mut tail_feq = initial_cash.to_f64();
    let mut fixed_realized = crate::money::Amount::ZERO;
    let mut fixed_qty = crate::money::Amount::ZERO;
    let mut fixed_entry = 0.0;
    // 破产告警只打一次, 避免逐 bar 刷屏。
    let mut bankrupt_warned = false;

    let mut trade_mfe: f64 = 0.0;
    let mut trade_mae: f64 = 0.0;

    // 持仓期止损状态。全部 `Option`: 未配置 / 已平仓时为 None, 不用 NaN 哨兵。
    // `pos_entry_at_close`: 本笔入场是否发生在 bar 收盘。收盘成交时当根
    // high/low 已在入场前走完, 故当根**不得**回溯触发止损。
    let mut pos_entry_at_close = false;
    let mut pos_stop_loss: Option<f64> = None;
    let mut pos_trail_extreme: Option<f64> = None;
    let mut pos_trail_stop: Option<f64> = None;
    let mut pos_strategy_stop: Option<f64> = None;
    let mut pos_breakeven_stop: Option<f64> = None;

    // 信号在 fill bar 才生效: 每个 bar 算出的目标方向要延迟 `delay` 根 bar 再应用。
    // 队列只保存尚未到期的目标(O(delay) 内存), 不物化 O(T) 的目标序列。
    // 声明在止损宏之前: 止损平仓时宏要把“stop 之前排队”的陈旧目标作废。
    let delay = exec.delay();
    let mut pending: std::collections::VecDeque<Side> =
        std::collections::VecDeque::with_capacity(delay.min(n));

    // 止损平仓与信号平仓共用同一套账本结算; 宏只负责把当前 bar 的止损
    // 状态、成交价与原因拼成 `CloseArgs`。用宏而不是闭包: 闭包会长期
    // 可变借用 `cash`/`acc`/`trades`, 与循环内其他读写冲突。
    macro_rules! try_stop_exit {
        ($bar:expr) => {{
            if cur_pos.is_open() {
                // 策略止损线是逐 bar 输出: 每根 bar 开始前取当前方向的价格线。
                // 无 line / NaN / 非正价格 => 该 bar 不设策略止损。
                pos_strategy_stop = match cur_pos {
                    Side::Long => strategy_stops.and_then(|s| s.long_at($bar)),
                    Side::Short => strategy_stops.and_then(|s| s.short_at($bar)),
                    Side::Flat => None,
                };
            }
            if cur_pos.is_open()
                && !($bar == pos_entry_bar && pos_entry_at_close)
                && let Some((base, reason)) = stop_exit_price(
                    $bar,
                    cur_pos,
                    high,
                    low,
                    open,
                    close,
                    pos_stop_loss,
                    pos_strategy_stop,
                    pos_trail_stop,
                    pos_breakeven_stop,
                )
            {
                let exit_price = fill_price(cur_pos.opposite(), base, exec.slip_rate);
                if valid_fill_price(exit_price) {
                    let cargs = CloseArgs {
                        cur_pos,
                        pos_qty,
                        pos_entry,
                        pos_entry_time,
                        pos_entry_bar,
                        exit_price,
                        exit_time: ts($bar),
                        exit_bar: $bar,
                        fixed_qty,
                        trade_mfe,
                        trade_mae,
                        fee_rate: exec.fee_rate,
                        reason,
                    };
                    close_at(
                        cargs,
                        &mut CloseState {
                            cash: &mut cash,
                            acc: &mut acc,
                            trades: &mut trades,
                            fixed_realized: &mut fixed_realized,
                            fixed_qty: &mut fixed_qty,
                            collect_curves,
                        },
                    )?;
                    cur_pos = Side::Flat;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                    pos_stop_loss = None;
                    pos_trail_extreme = None;
                    pos_trail_stop = None;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
                    // 止损是风险覆盖, 不是策略信号: 平仓后把目标重置为 Flat,
                    // 并把 stop 之前已经排队的陈旧目标全部改成 Flat。**不能
                    // clear**: 队列长度编码了 delay 的时序, 清空会让下一根
                    // 新信号提前成交; 用 Flat 占位既保持长度, 又不会重入。
                    // 只有止损 bar 及之后新出现的 Buy/Sell 才会重新入场。
                    target = Side::Flat;
                    for slot in &mut pending {
                        *slot = Side::Flat;
                    }
                }
            }
        }};
    }

    // 旧实现把 `cur_pos`/`cash` 在**信号 bar** 就改掉、却用 `i + delay` 的价格成交,
    // 于是权益曲线在成交前就按未来价格盯市 —— Sharpe / 回撤 / Sortino 全部被污染。
    // 现在所有状态变更都推迟到 fill bar, 曲线在 bar i 只反映 bar i 及之前已发生的成交。
    for i in 0..n {
        // 0) `fill_close=true` 时, 到期信号在收盘才成交; 盘中 stop 可能先触发。
        //    先查 stop, 再执行收盘信号, 避免用收盘价"跳过"盘中已经发生的止损。
        if exec.fill_close {
            try_stop_exit!(i);
        }

        // 1) 先处理本 bar 到期的成交(信号 bar `i - delay` 的目标方向)。
        if i >= delay
            && let Some(due) = pending.pop_front()
        {
            // 反手 / 平仓: 先按本 bar 成交价了结旧仓。
            if cur_pos.is_open() && cur_pos != due {
                let exit_side = cur_pos.opposite();
                // 成交价非法(非有限/非正)时 `fill_at` 返回 None —— 此时**放弃
                // 本次平仓**(持仓延续到下一根), 而不是用会污染全部指标的
                // NaN/0 价格结算。
                if let Some(exit_price) = fill_at(i, exit_side, close, open, exec) {
                    let cargs = CloseArgs {
                        cur_pos,
                        pos_qty,
                        pos_entry,
                        pos_entry_time,
                        pos_entry_bar,
                        exit_price,
                        exit_time: ts(i),
                        exit_bar: i,
                        fixed_qty,
                        trade_mfe,
                        trade_mae,
                        fee_rate: exec.fee_rate,
                        reason: ExitReason::Signal,
                    };
                    close_at(
                        cargs,
                        &mut CloseState {
                            cash: &mut cash,
                            acc: &mut acc,
                            trades: &mut trades,
                            fixed_realized: &mut fixed_realized,
                            fixed_qty: &mut fixed_qty,
                            collect_curves,
                        },
                    )?;
                    cur_pos = Side::Flat;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                    pos_stop_loss = None;
                    pos_trail_extreme = None;
                    pos_trail_stop = None;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
                }
            }

            // 开仓(含反手后的立刻反向开仓)。平仓失败时 `cur_pos` 仍持仓, 这里
            // 不会开新仓, 从而保证同一 bar 不会同时持有多空两条腿。
            //
            // **破产保护**: 现金 <= 0 时禁止开仓。成交量 = `cash / price`, 现金为负
            // 会得到负的数量 —— 空头"负负得正"变成做多、多头则越亏越多, 且现金
            // 永远无法回正。旧实现没有这道闸门, 爆仓后仍可无限开仓。
            if !cur_pos.is_open() && due.is_open() {
                if cash <= crate::money::Amount::ZERO {
                    if !bankrupt_warned {
                        tracing::warn!(
                            bar = i,
                            cash = %cash,
                            "现金 <= 0, 拒绝再开仓(账户已破产)"
                        );
                        bankrupt_warned = true;
                    }
                } else if let Some(entry_price) = fill_at(i, due, close, open, exec)
                    && let Some((qty, fixed_qty_entry)) = entry_qty(
                        sizing,
                        &acc,
                        sizing_atr.as_deref(),
                        i,
                        exec.fill_close,
                        entry_price,
                        cash,
                        initial_cash,
                    )?
                {
                    let fee = qty.mul_price(entry_price)?.mul_ratio(exec.fee_rate)?;
                    pos_entry = entry_price;
                    pos_entry_time = ts(i);
                    pos_entry_bar = i;
                    pos_qty = qty;
                    fixed_qty = fixed_qty_entry;
                    fixed_entry = entry_price;
                    let notional = qty.mul_price(entry_price)?;
                    if due == Side::Long {
                        cash = cash.minus(notional.plus(fee)?)?;
                    } else {
                        cash = cash.plus(notional.minus(fee)?)?;
                    }
                    cur_pos = due;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                    // 收盘成交时, 本根 high/low 发生在入场之前, 不得回溯触发止损。
                    pos_entry_at_close = exec.fill_close || open.get(i).is_none();
                    let pct_stop = |pct: f64| {
                        if due == Side::Long {
                            entry_price * (1.0 - pct / 100.0)
                        } else {
                            entry_price * (1.0 + pct / 100.0)
                        }
                    };
                    let atr_entry = risk.atr.and_then(|_| {
                        atr_values
                            .as_deref()
                            .and_then(|atr| atr_at_entry(atr, i, exec.fill_close))
                    });
                    let atr_stop = |cfg: AtrStopConfig| {
                        atr_entry.map(|atr| {
                            if due == Side::Long {
                                entry_price - cfg.mult * atr
                            } else {
                                entry_price + cfg.mult * atr
                            }
                        })
                    };
                    // 固定止损: 百分比与 ATR 固定止损取更紧的一侧。
                    let atr_fixed = risk.atr.filter(|c| !c.trailing).and_then(atr_stop);
                    pos_stop_loss = tighter_stop(due, risk.stop_loss_pct.map(pct_stop), atr_fixed);
                    // 追踪止损: 百分比与 ATR 追踪止损共用同一个有利极值。
                    let atr_trail = risk.atr.filter(|c| c.trailing).and_then(atr_stop);
                    let trail = tighter_stop(due, risk.trailing_stop_pct.map(pct_stop), atr_trail);
                    // 即使初始 ATR 尚不可用, 也要先记录极值; 后续 bar 的 ATR
                    // 一旦可用, 追踪止损即可启动。
                    pos_trail_extreme = (risk.trailing_stop_pct.is_some()
                        || risk.atr.is_some_and(|c| c.trailing))
                    .then_some(entry_price);
                    pos_trail_stop = trail;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
                }
            }
        }

        // `fill_close=false` 时, 到期信号已在开盘成交; 新仓在**同一根 bar**
        // 内就可能触发止损(开盘成交, 盘中 high/low 尚未走完)。
        if !exec.fill_close {
            try_stop_exit!(i);
        }

        // 2) 浮动盈亏只统计已成交持仓(fill bar 之后), fill 前的漂移不计入 MFE/MAE。
        if cur_pos.is_open() {
            let favor = if cur_pos == Side::Long {
                close[i] - pos_entry
            } else {
                pos_entry - close[i]
            };
            if favor > trade_mfe {
                trade_mfe = favor;
            }
            let adverse = -favor;
            if adverse > trade_mae {
                trade_mae = adverse;
            }
        }

        // 3) 权益曲线是**序列**(供 `CurveAccum` 算对数收益、供 PBO 的 CSCV 用),
        // 需要超越函数 ⇒ 仍是 `f64`; 但它的**来源**是定点十进制账本。
        //
        // **价格非有限时按"已实现权益"计**(而不是让金额运算报错): 框架的既有
        // 契约是"非法价格 ⇒ 放弃成交 ⇒ 指标退化为 0 而非 NaN"(P7), 整轮回测
        // **不应**因一根 NaN bar 而失败。旧实现这里会产出 NaN 权益(被
        // `log_ret` 的定义域保护吃掉, 记 0 收益); 现在退化为"现金 + 浮动盈亏
        // 不可估 ⇒ 只记现金" —— 同样是有限的, 且不再把 NaN 写进曲线。
        let px = close[i];
        let eq_amt = match cur_pos {
            Side::Long if px.is_finite() => cash.plus(pos_qty.mul_price(px)?)?,
            Side::Short if px.is_finite() => cash.minus(pos_qty.mul_price(px)?)?,
            _ => cash,
        };
        let eq = eq_amt.to_f64();
        let feq = match cur_pos {
            Side::Long if px.is_finite() => initial_cash
                .plus(fixed_realized)?
                .plus(fixed_qty.mul_price(px - fixed_entry)?)?,
            Side::Short if px.is_finite() => initial_cash
                .plus(fixed_realized)?
                .plus(fixed_qty.mul_price(fixed_entry - px)?)?,
            _ => initial_cash.plus(fixed_realized)?,
        }
        .to_f64();
        if i + 1 < n {
            eq_acc.push(eq);
            fx_acc.push(feq);
            if collect_curves {
                equity_curve.push(eq);
                fixed_curve.push(feq);
            }
        } else {
            tail_eq = eq;
            tail_eq_amt = eq_amt;
            tail_feq = feq;
        }

        // 3b) 追踪止损只用**已收盘 bar** 的 high/low 更新 extreme, 供下一根
        //     bar 判定; 当根入场且为收盘成交时跳过, 避免把入场前的行情算进去。
        let trail_atr = risk.atr.filter(|c| c.trailing);
        if cur_pos.is_open() && (risk.trailing_stop_pct.is_some() || trail_atr.is_some()) {
            if !(i == pos_entry_bar && pos_entry_at_close) {
                let h = high.get(i).copied().unwrap_or(close[i]);
                let l = low.get(i).copied().unwrap_or(close[i]);
                if let Some(ext) = pos_trail_extreme {
                    let next = match cur_pos {
                        Side::Long if h.is_finite() => ext.max(h),
                        Side::Short if l.is_finite() => ext.min(l),
                        _ => ext,
                    };
                    pos_trail_extreme = Some(next);
                }
            }
            if let Some(ext) = pos_trail_extreme {
                let pct_stop = risk.trailing_stop_pct.map(|pct| {
                    if cur_pos == Side::Long {
                        ext * (1.0 - pct / 100.0)
                    } else {
                        ext * (1.0 + pct / 100.0)
                    }
                });
                let atr_stop = trail_atr.and_then(|cfg| {
                    let atr = atr_values
                        .as_ref()
                        .and_then(|values| values.get(i))
                        .copied()
                        .filter(|v| v.is_finite() && *v > 0.0)?;
                    Some(if cur_pos == Side::Long {
                        ext - cfg.mult * atr
                    } else {
                        ext + cfg.mult * atr
                    })
                });
                // 追踪止损只能收紧: ATR 放大时 `extreme - mult*ATR` 可能比
                // 上一根更松, 必须与已有止损取更紧的一侧, 否则止损会向后
                // 退让。百分比追踪本身单调, 与旧值合并不改变结果。
                let candidate = tighter_stop(cur_pos, pct_stop, atr_stop);
                if let Some(stop) = tighter_stop(cur_pos, pos_trail_stop, candidate) {
                    pos_trail_stop = Some(stop);
                }
            }
        }

        // 3c) 保本止损: 收盘价有利幅度达到 trigger 后, 把止损移到入场价
        //     加减 offset。收盘后更新, 下一根 bar 才生效, 不使用当根 high/low。
        if cur_pos.is_open()
            && let Some(be) = risk.breakeven
            && !(i == pos_entry_bar && pos_entry_at_close)
            && pos_entry.is_finite()
            && pos_entry > 0.0
            && close[i].is_finite()
        {
            let favorable_pct = if cur_pos == Side::Long {
                (close[i] - pos_entry) / pos_entry * 100.0
            } else {
                (pos_entry - close[i]) / pos_entry * 100.0
            };
            if favorable_pct >= be.trigger_pct {
                let candidate = if cur_pos == Side::Long {
                    pos_entry * (1.0 + be.offset_pct / 100.0)
                } else {
                    pos_entry * (1.0 - be.offset_pct / 100.0)
                };
                pos_breakeven_stop = tighter_stop(cur_pos, pos_breakeven_stop, Some(candidate));
            }
        }

        // 4) 本 bar 的信号只影响 `i + delay` 的成交, 先算出新目标并排队。
        target = next_target(target, signals[i]);
        pending.push_back(target);
    }

    if cur_pos.is_open() {
        // 末根强制平仓。成交价非法(非有限/非正)时不强行结算 —— 宁可留一笔未平仓
        // 持仓(末根权益按盯市值计), 也不用 NaN 污染全部指标。
        let last_price = fill_price(cur_pos.opposite(), close[n - 1], exec.slip_rate);
        if valid_fill_price(last_price) {
            let cargs = CloseArgs {
                cur_pos,
                pos_qty,
                pos_entry,
                pos_entry_time,
                pos_entry_bar,
                exit_price: last_price,
                exit_time: ts(n - 1),
                exit_bar: n - 1,
                fixed_qty,
                trade_mfe,
                trade_mae,
                fee_rate: exec.fee_rate,
                reason: ExitReason::EndOfData,
            };
            close_at(
                cargs,
                &mut CloseState {
                    cash: &mut cash,
                    acc: &mut acc,
                    trades: &mut trades,
                    fixed_realized: &mut fixed_realized,
                    fixed_qty: &mut fixed_qty,
                    collect_curves,
                },
            )?;
            // 强制平仓: 末根权益由盯市值改写为已实现 cash(扣掉平仓费与滑点)。
            tail_eq = cash.to_f64();
            tail_eq_amt = cash;
            tail_feq = initial_cash.plus(fixed_realized)?.to_f64();
        }
    }

    if n > 0 {
        eq_acc.push(tail_eq);
        fx_acc.push(tail_feq);
        if collect_curves {
            equity_curve.push(tail_eq);
            fixed_curve.push(tail_feq);
        }
    }

    Ok(ScanOut {
        metrics: MetricsSource {
            acc,
            eq: eq_acc,
            fx: fx_acc,
            final_equity: tail_eq_amt,
        },
        detail: DetailArtifacts {
            trades,
            equity: equity_curve,
            fixed: fixed_curve,
        },
        collect_curves,
    })
}

/// 单根 bar 的对数收益, 含**定义域保护**(两侧都要查)。
///
/// 空头持仓期间权益可能 ≤ 0(浮空)。此时比值 `curr/prev` 为负, `ln()` 返回 NaN;
/// NaN 经 Welford 污染 `ret_mean`/`ret_m2` 后会让 `ret_m2 > 0.0` 守卫失效,
/// 于是 sharpe/volatility 被**静默归零**(比 NaN 更危险: NaN 会暴露, 0 会被读成
/// "零波动低风险"), 同时 sortino 直接输出 NaN。故 `prev <= 0`、`curr <= 0`
/// 或任一侧非有限时记 0(退化而非污染)。
///
/// 注意: 只查 `prev` 不够 —— 首根即浮空(空头开仓当根权益转负)时 `prev` 仍为正,
/// `curr/prev < 0` 依旧产出 NaN。
#[inline]
fn log_ret(prev: f64, curr: f64) -> f64 {
    if !prev.is_finite() || !curr.is_finite() || prev <= 0.0 || curr <= 0.0 {
        0.0
    } else {
        (curr / prev).ln()
    }
}

/// 权益曲线 → 逐 bar 对数收益。引擎全部夏普口径的唯一来源; `pbo.rs::to_returns`
/// 也必须调用本函数, 否则 CSCV 的 IS/OOS 排序与引擎排名不同源。
///
/// 额外暴露 `Clone` 边界: 调用方需要在**不物化**收益数组的前提下做两遍统计
/// (先求均值、再求离差平方和), 逐位等价于「物化后单遍遍历」, 但省掉 O(T) 内存。
pub fn bar_log_returns(curve: &[f64]) -> impl ExactSizeIterator<Item = f64> + Clone + '_ {
    curve.windows(2).map(|w| log_ret(w[0], w[1]))
}

/// 权益曲线的**在线**统计累加器: O(1) 内存, 逐 bar [`CurveAccum::push`]。
///
/// 与「物化曲线后单遍遍历」**逐位一致**, 故 [`simulate`] 可省掉 O(T) 的曲线内存。
///
/// 动机: `simulate()` 只需要统计量, 不需要曲线本身。物化整条曲线
/// (`Vec<f64>`, 每 bar 8 B; 复利腿 + fixed 腿 → T×16 B) 后再单遍扫描,
/// 在 T = 1M 时每参数多占 16 MB。参数网格是**串行**外循环(见 README
/// 「关键设计约束」第 1 条, 策略层并行会 OOM), 故峰值内存直接决定 OOM 边界。
///
/// 正确性: 在线累计与「物化曲线后单遍遍历」**逐位一致**, 由
/// `test_curve_accum_is_bit_identical_to_materialized_reference` 对 5 类曲线
/// (单调上涨 / 锯齿 / 首根即持仓 / 穿零 / 含 NaN) 锁定。故这是纯内存优化,
/// **不改变任何指标数值**。
#[derive(Debug, Clone)]
pub struct CurveAccum {
    initial_cash: f64,
    /// 已喂入的点数(含初始点)。
    n: usize,
    ret_mean: f64,
    ret_m2: f64,
    ret_m2_downside: f64,
    n_downside: f64,
    peak: f64,
    max_dd: f64,
    last: f64,
}

/// 曲线统计结果。复利腿与 fixed 腿共用同一口径, 保证两套 Sharpe 可比。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveStats {
    /// 年化夏普。
    pub sharpe: f64,
    /// Sortino 比率(下行偏差口径, ddof=1)。
    pub sortino: f64,
    /// 最大回撤(小数)。
    pub max_dd: f64,
    /// 年化波动率, 百分数。
    pub volatility: f64,
    /// 总收益率(小数, 非百分数)。
    pub total_return: f64,
}

impl TradeAccum {
    /// 记一笔已平仓交易。
    ///
    /// `pnls` 必须保留: `compute_metrics` 的 SQN / skewness / kurtosis /
    /// pf-stability 分半统计都需要逐笔 pnl。其余字段都可以从它导出, 故不重复存。
    fn push(&mut self, o: CloseOutcome) -> crate::error::Result<()> {
        self.num_trades += 1;
        match o.reason {
            ExitReason::StopLoss => self.stop_loss_exits += 1,
            ExitReason::TrailingStop => self.trailing_stop_exits += 1,
            ExitReason::StrategyStop => self.strategy_stop_exits += 1,
            ExitReason::Signal | ExitReason::EndOfData => {}
        }
        self.pnls.push(o.pnl.to_f64());
        self.fixed_pnls.push(o.fixed_pnl.to_f64());
        self.pnl_pcts.push(o.pnl_pct);
        self.total_fixed_pnl = self
            .total_fixed_pnl
            .plus(o.fixed_pnl)
            .map_err(|e| crate::error::AppError::Msg(format!("累计 fixed pnl 溢出: {e}")))?;
        if o.pnl > crate::money::Amount::ZERO {
            self.total_win = self
                .total_win
                .plus(o.pnl)
                .map_err(|e| crate::error::AppError::Msg(format!("累计盈利溢出: {e}")))?;
        } else if o.pnl < crate::money::Amount::ZERO {
            // 保本(pnl == 0)既不计盈利也不计亏损: 旧实现走 `else` 分支,
            // 把 0 记成一次"亏损"(金额为 0), 于是 avg_loss 的分母被稀释,
            // 胜率与连亏也被拉低/拉长。
            self.total_loss = self
                .total_loss
                .plus(o.pnl.abs())
                .map_err(|e| crate::error::AppError::Msg(format!("累计亏损溢出: {e}")))?;
        }
        self.pnl_pct_sum += o.pnl_pct;
        self.sum_mfe += o.max_runup;
        self.sum_mae += o.max_drawdown;
        self.sum_hold += o.bars_held as f64;
        Ok(())
    }
}

impl CurveAccum {
    /// 以初始资金建立累加器。
    #[must_use]
    pub fn new(initial_cash: f64) -> Self {
        Self {
            initial_cash,
            n: 0,
            ret_mean: 0.0,
            ret_m2: 0.0,
            ret_m2_downside: 0.0,
            n_downside: 0.0,
            peak: initial_cash,
            max_dd: 0.0,
            last: initial_cash,
        }
    }

    /// 喂入一根 bar 的权益值。
    ///
    /// 首点只登记基线, **不参与** peak/max_dd 统计 —— 与「`peak` 初值为
    /// `initial_cash` 且循环从 index 1 起」的物化口径一致。
    pub fn push(&mut self, eq: f64) {
        if self.n == 0 {
            self.n = 1;
            self.last = eq;
            return;
        }
        let ret = log_ret(self.last, eq);
        let k = self.n as f64;
        let delta = ret - self.ret_mean;
        self.ret_mean += delta / k;
        self.ret_m2 += delta * (ret - self.ret_mean);
        if ret < 0.0 {
            self.ret_m2_downside += ret * ret;
            self.n_downside += 1.0;
        }
        if eq > self.peak {
            self.peak = eq;
        }
        let dd = (self.peak - eq) / self.peak;
        if dd > self.max_dd {
            self.max_dd = dd;
        }
        self.last = eq;
        self.n += 1;
    }

    /// 已喂入的点数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否尚未喂入任何点。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 末点权益。
    #[must_use]
    pub fn last(&self) -> f64 {
        self.last
    }

    /// 逐 bar 对数收益口径的全部曲线统计。
    #[must_use]
    pub fn stats(&self, bars_per_year: f64) -> CurveStats {
        let num_bars = (self.n.saturating_sub(1)) as f64;
        let total_return = if self.initial_cash > 0.0 {
            (self.last - self.initial_cash) / self.initial_cash
        } else {
            0.0
        };
        if self.n < 2 {
            return CurveStats {
                sharpe: 0.0,
                sortino: 0.0,
                max_dd: 0.0,
                volatility: 0.0,
                total_return,
            };
        }
        // `is_finite` 守卫不可省: 见 [`log_ret`] 的说明 —— NaN 会让 `> 0.0` 为假,
        // 但 `-inf` / 极大值会让它为真, 从而把非有限值推进方差。
        let (sharpe, volatility) = if self.ret_m2.is_finite() && self.ret_m2 > 0.0 && num_bars > 1.0
        {
            let variance = self.ret_m2 / (num_bars - 1.0);
            (
                self.ret_mean / variance.sqrt() * bars_per_year.sqrt(),
                variance.sqrt() * bars_per_year.sqrt() * 100.0,
            )
        } else {
            (0.0, 0.0)
        };
        // 下行偏差按**下行 bar 数**归一(ddof=1), 与 sharpe 同一自由度约定。
        let sortino = if self.ret_m2_downside.is_finite()
            && self.ret_m2_downside > 0.0
            && self.n_downside > 1.0
        {
            let downside_var = self.ret_m2_downside / (self.n_downside - 1.0);
            self.ret_mean / downside_var.sqrt() * bars_per_year.sqrt()
        } else {
            0.0
        };
        CurveStats {
            sharpe,
            sortino,
            max_dd: self.max_dd,
            volatility,
            total_return,
        }
    }
}

/// 仅测试用: 「物化曲线后单遍遍历」的参照实现。
///
/// [`CurveAccum`] 的正确性基准。它刻意用与累加器**不同**的写法
/// (先物化 `Vec`、再用索引遍历), 使"两者同时写错同一处"不可能发生。
#[cfg(test)]
fn reference_curve_stats(curve: &[f64], initial_cash: f64, bars_per_year: f64) -> CurveStats {
    let n = curve.len();
    if n < 2 {
        return CurveStats {
            sharpe: 0.0,
            sortino: 0.0,
            max_dd: 0.0,
            volatility: 0.0,
            total_return: 0.0,
        };
    }
    let mut ret_mean = 0.0;
    let mut ret_m2 = 0.0;
    let mut ret_m2_downside = 0.0;
    let mut n_downside = 0.0;
    let mut peak = initial_cash;
    let mut max_dd = 0.0;
    for i in 1..n {
        let ret = bar_log_returns(&curve[i - 1..i + 1]).next().unwrap_or(0.0);
        let delta = ret - ret_mean;
        ret_mean += delta / i as f64;
        ret_m2 += delta * (ret - ret_mean);
        if ret < 0.0 {
            ret_m2_downside += ret * ret;
            n_downside += 1.0;
        }
        if curve[i] > peak {
            peak = curve[i];
        }
        let dd = (peak - curve[i]) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    let num_bars = (n - 1) as f64;
    let (sharpe, volatility) = if ret_m2.is_finite() && ret_m2 > 0.0 && num_bars > 1.0 {
        let variance = ret_m2 / (num_bars - 1.0);
        (
            ret_mean / variance.sqrt() * bars_per_year.sqrt(),
            variance.sqrt() * bars_per_year.sqrt() * 100.0,
        )
    } else {
        (0.0, 0.0)
    };
    let sortino = if ret_m2_downside.is_finite() && ret_m2_downside > 0.0 && n_downside > 1.0 {
        let downside_var = ret_m2_downside / (n_downside - 1.0);
        ret_mean / downside_var.sqrt() * bars_per_year.sqrt()
    } else {
        0.0
    };
    let final_eq = *curve.last().unwrap_or(&initial_cash);
    CurveStats {
        sharpe,
        sortino,
        max_dd,
        volatility,
        total_return: (final_eq - initial_cash) / initial_cash,
    }
}

/// 由**唯一**指标源计算全部 `SummaryMetrics`。
///
/// 签名只收 [`MetricsSource`] —— 落盘产物(`trades` / 曲线)在类型上不可达, 故
/// 指标**不可能**依赖 `--detail` 开关。
fn compute_metrics(
    out: &MetricsSource,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
) -> crate::error::Result<SummaryMetrics> {
    let n = out.eq.len();
    if n < 2 {
        return Ok(SummaryMetrics::default());
    }

    // 曲线统计全部来自在线累加器(与物化口径逐位一致, 见 CurveAccum 的文档)。
    let st = out.eq.stats(bars_per_year);
    // 末点权益取**账本精确值**, 而不是 f64 曲线的最后一点: 后者再经
    // `Amount::from_f64` 回造会丢掉 15 位之后的有效数字, 与逐笔 pnl 之和对不上。
    let final_eq = out.final_equity.to_f64();
    let total_return = if initial_cash > crate::money::Amount::ZERO {
        out.final_equity.minus(initial_cash)?.to_f64() / initial_cash.to_f64()
    } else {
        0.0
    };
    let max_dd = st.max_dd;
    let pnls = &out.acc.pnls;

    let num_bars = (n - 1) as f64;
    let years = if bars_per_year > 0.0 {
        num_bars / bars_per_year
    } else {
        0.0
    };
    let sharpe = st.sharpe;
    let sortino = st.sortino;
    let volatility = st.volatility;

    let cagr = if bars_per_year > 0.0 && n > 1 {
        let years = num_bars / bars_per_year;
        if years > 0.0 && initial_cash.to_f64() > 0.0 && final_eq > 0.0 {
            // CAGR 是**比率**(无量纲)⇒ f64; 金额相除时取各自的 f64 视图。
            (final_eq / initial_cash.to_f64()).powf(1.0 / years) - 1.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let calmar = if max_dd > 0.0 { cagr / max_dd } else { 0.0 };

    let recovery_factor = if max_dd > 0.0 {
        total_return / max_dd
    } else {
        0.0
    };

    let num_trades = out.acc.num_trades;
    // avg_mae / avg_mfe / avg_hold_bars 都进报告列, 故 TradeAccum 同时累计
    // 这三个标量 —— 不必为它们保留整个 Vec<Trade>。
    let mut num_wins = 0u32;
    let mut num_losses = 0u32;
    // 金额累计走 `Amount`(定点十进制); 下面的 `pnls` 序列是**统计量**口径(f64)。
    let total_win = out.acc.total_win;
    let total_loss = out.acc.total_loss;
    let mut cw = 0u32;
    let mut cl = 0u32;
    let mut max_cw = 0u32;
    let mut max_cl = 0u32;

    let mut pnl_m2 = 0.0;
    let mut pnl_m3 = 0.0;
    let mut pnl_m4 = 0.0;
    let mut runs = 0u32;
    let mut prev_win: Option<bool> = None;

    let nt = num_trades as f64;
    let pnl_mean = if nt > 0.0 {
        pnls.iter().sum::<f64>() / nt
    } else {
        0.0
    };

    for &p in pnls.iter() {
        // 保本(p == 0)既不算赢也不算亏, 且**不打断**连胜/连亏段; runs 检验
        // 也直接跳过它(带并列值的符号序列没有唯一的 runs 定义, 忽略是标准做法)。
        if p > 0.0 {
            num_wins += 1;
            cw += 1;
            cl = 0;
            if cw > max_cw {
                max_cw = cw;
            }
        } else if p < 0.0 {
            num_losses += 1;
            cl += 1;
            cw = 0;
            if cl > max_cl {
                max_cl = cl;
            }
        }
        if p != 0.0 {
            let cur_win = p > 0.0;
            match prev_win {
                None => runs = 1,
                Some(prev) if prev != cur_win => runs += 1,
                _ => {}
            }
            prev_win = Some(cur_win);
        }

        let d = p - pnl_mean;
        let d2 = d * d;
        pnl_m2 += d2;
        pnl_m3 += d2 * d;
        pnl_m4 += d2 * d2;
    }

    let avg_win = if num_wins > 0 {
        total_win.div_count(i64::from(num_wins))?
    } else {
        crate::money::Amount::ZERO
    };
    let avg_loss = if num_losses > 0 {
        total_loss.div_count(i64::from(num_losses))?
    } else {
        crate::money::Amount::ZERO
    };
    // 下面的比值类指标是**无量纲比率** ⇒ f64。
    let profit_factor = if total_loss > crate::money::Amount::ZERO {
        total_win.to_f64() / total_loss.to_f64()
    } else {
        total_win.to_f64().max(0.0)
    };
    let expectancy = if num_trades > 0 {
        out.acc.pnl_pct_sum / f64::from(num_trades)
    } else {
        0.0
    };

    let avg_mae = if nt > 0.0 { out.acc.sum_mae / nt } else { 0.0 };
    let avg_mfe = if nt > 0.0 { out.acc.sum_mfe / nt } else { 0.0 };
    let edge_ratio = if avg_mae > 0.0 {
        avg_mfe / avg_mae
    } else {
        0.0
    };
    let avg_hold_bars = if nt > 0.0 { out.acc.sum_hold / nt } else { 0.0 };

    let wl_ratio = if avg_loss > crate::money::Amount::ZERO {
        avg_win.to_f64() / avg_loss.to_f64()
    } else {
        0.0
    };

    let skewness = if nt > 2.0 && pnl_m2 > 0.0 {
        // 总体矩 m2 = Σd²/n、m3 = Σd³/n; g1 = m3 / m2^1.5 是总体偏度。
        // 再乘 Fisher-Pearson 小样本校正 √(n(n-1))/(n-2)。
        // 旧实现把**样本**标准差 √(m2/(n-1)) 当作 m2^0.5, 结果比标准校正偏度
        // 小 ((n-1)/n)^1.5(n=10 时约低 14%)。
        let m2 = pnl_m2 / nt;
        let m3 = pnl_m3 / nt;
        let g1 = m3 / m2.powf(1.5);
        g1 * (nt * (nt - 1.0)).sqrt() / (nt - 2.0)
    } else {
        0.0
    };

    let kurtosis = if nt > 3.0 && pnl_m2 > 0.0 {
        // 总体矩 m2/m4 + Fisher 小样本校正:
        //   ((n²-1) / ((n-2)(n-3))) · m4/m2² - 3(n-1)² / ((n-2)(n-3))
        // 旧实现在第一项多乘了一个 n, 峰度被系统性放大。
        let n = nt;
        let m2 = pnl_m2 / n;
        let m4 = pnl_m4 / n;
        ((n * n - 1.0) / ((n - 2.0) * (n - 3.0))) * (m4 / (m2 * m2))
            - 3.0 * (n - 1.0) * (n - 1.0) / ((n - 2.0) * (n - 3.0))
    } else {
        0.0
    };

    let sqn = if nt > 1.0 && pnl_m2 > 0.0 {
        let pnl_std = (pnl_m2 / (nt - 1.0)).sqrt();
        if pnl_std > 0.0 {
            // SQN = √n · mean/stdev (Van Thorp, "The Risk of Ruin" 1998)。
            // √n 是定义中**有意的**统计显著性因子, 不可封顶: 原实现用 min(n,100)
            // 会让 n=200 与 n=20000 得到相同 SQN, 抹掉"证据量"这一维度。
            // 小样本的可读性由"配对 num_trades 列 + 门槛"解决(Van Thorp 建议 n>=30)。
            pnl_mean / pnl_std * nt.sqrt()
        } else {
            0.0
        }
    } else {
        0.0
    };

    let pf_stability = {
        let half = num_trades as usize / 2;
        if half >= 10 {
            let pf1 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[..half] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let pf2 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[half..] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let (mn, mx) = if pf1 < pf2 { (pf1, pf2) } else { (pf2, pf1) };
            if mx > 0.0 { mn / mx } else { 0.0 }
        } else {
            0.0
        }
    };

    let (z_score, p_value) = if num_trades > 1 && num_wins > 0 && num_losses > 0 {
        let nw = num_wins as f64;
        let nl = num_losses as f64;
        let n = num_trades as f64;
        let e_runs = 2.0 * nw * nl / n + 1.0;
        let var_runs = 2.0 * nw * nl * (2.0 * nw * nl - n) / (n * n * (n - 1.0));
        let std_runs = var_runs.sqrt();
        let z = if std_runs > 0.0 {
            (runs as f64 - e_runs) / std_runs
        } else {
            0.0
        };
        let p = 2.0 * (1.0 - normal_cdf(z.abs()));
        (z, p)
    } else {
        (0.0, 1.0)
    };

    tracing::debug!(
        num_trades,
        total_return_pct = total_return * 100.0,
        sharpe_ratio = sharpe,
        max_drawdown_pct = max_dd * 100.0,
        sqn,
        "metrics computed"
    );

    // fixed 腿统计: 以逐笔 fixed_pnl 与 fixed 盯市曲线为准, 与复利腿同公式可比。
    // 研究报告统一引用 fixed_*; 无后缀的复利口径仅供容量参考。
    let mut fixed_win = 0.0;
    let mut fixed_loss = 0.0;
    // fixed 腿的 win/loss 拆分。非 detailed 路径没有 Trade, 但逐笔 fixed_pnl
    // 可以从 acc 复原 —— 故把 fixed_pnl 一并留在 CloseOutcome 里。
    for &f in out.acc.fixed_pnls.iter() {
        if f > 0.0 {
            fixed_win += f;
        } else if f < 0.0 {
            fixed_loss += f.abs();
        }
    }
    let fixed_pf = if fixed_loss > 0.0 {
        fixed_win / fixed_loss
    } else {
        fixed_win.max(0.0)
    };
    let fixed_expectancy_bps = if nt > 0.0 {
        out.acc.total_fixed_pnl.to_f64() / nt / initial_cash.to_f64() * 10_000.0
    } else {
        0.0
    };
    let fx_st = out.fx.stats(bars_per_year);
    Ok(SummaryMetrics {
        total_return_pct: total_return * 100.0,
        total_return,
        sharpe_ratio: sharpe,
        sortino_ratio: sortino,
        max_drawdown_pct: max_dd * 100.0,
        // 胜率分母是**已决笔数**(胜 + 负), 保本不进分母: 把零盈亏算作
        // 一次非胜交易会系统性拉低胜率, 与"排除出亏损统计"的口径自相矛盾。
        win_rate: {
            let decided = num_wins + num_losses;
            if decided > 0 {
                num_wins as f64 / f64::from(decided) * 100.0
            } else {
                0.0
            }
        },
        profit_factor,
        calmar_ratio: calmar,
        num_trades,
        stop_loss_exits: out.acc.stop_loss_exits,
        trailing_stop_exits: out.acc.trailing_stop_exits,
        strategy_stop_exits: out.acc.strategy_stop_exits,
        avg_win,
        avg_loss,
        expectancy,
        consecutive_wins: max_cw,
        consecutive_losses: max_cl,
        final_equity: out.final_equity,
        volatility,
        recovery_factor,
        avg_hold_bars,
        avg_mae,
        avg_mfe,
        edge_ratio,
        wl_ratio,
        skewness,
        kurtosis,
        sqn,
        pf_stability,
        z_score,
        p_value,
        total_fixed_pnl: out.acc.total_fixed_pnl,
        turnover_per_year: if years > 0.0 {
            f64::from(num_trades) / years
        } else {
            0.0
        },
        // 执行口径由 simulate()/落盘层回填, 裸 compute 为旧口径 0。
        delay_bars: 0,
        fee_bps: 0.0,
        slippage_bps: 0.0,
        fixed_total_return: fx_st.total_return,
        fixed_sharpe_ratio: fx_st.sharpe,
        fixed_max_drawdown_pct: fx_st.max_dd * 100.0,
        fixed_expectancy_bps,
        fixed_profit_factor: fixed_pf,
    })
}
fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    sign * y
}

/// 执行一次回测并返回全部指标。
///
/// **不物化权益曲线**(曲线统计 O(1))。`--detail` 开关不影响任何指标数值,
/// 详见 `crate::lib` 的不变量 2。
///
/// 注意: 逐笔 pnl 序列仍需保留用于 SQN/偏度/峰度/分半 PF, 最坏 O(交易笔数),
/// 但远小于「每条 bar 两条 f64 曲线」的 O(T) 物化成本。
pub fn simulate(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc(
        close,
        close,
        close,
        open,
        signals,
        initial_cash,
        bars_per_year,
        exec,
        &StopConfig::default(),
    )
}

/// 带盘中 high/low 与持仓期止损的回测。
///
/// `high` / `low` 必须与 `close` 同长(生产路径由 `KlineColumns` 保证); 缺失时
/// 止损判定退回收盘价, 仅用于兼容旧调用。未配置 `risk` 时, `high` / `low`
/// 完全不参与计算, 结果与 [`simulate`] 逐位一致。
///
/// # Errors
/// 执行费率非法、止损百分比越界, 或配置了止损但 `high` / `low` 短于
/// `close` 时返回错误。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc_with_stops(
        high,
        low,
        close,
        open,
        signals,
        None,
        initial_cash,
        bars_per_year,
        exec,
        risk,
    )
}

/// 带策略止损线的盘中回测。
///
/// 与 [`simulate_ohlc`] 相同, 额外接受策略输出的 [`StopLines`]。策略止损线
/// 与引擎 [`StopConfig`] 可以同时存在, 引擎取更紧的一侧; 策略止损触发时
/// 平仓原因记为 [`ExitReason::StrategyStop`]。
///
/// # Errors
/// 执行费率非法、止损参数越界、策略止损线长度不足, 或配置了任一种盘中
/// 止损但 `high` / `low` 短于 `close` 时返回错误。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc_with_stops_and_sizing(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
) -> crate::error::Result<SummaryMetrics> {
    exec.validate_rates()?;
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let needs_ohlc =
        !risk.is_none() || strategy_stops.is_some_and(|s| !s.is_none()) || sizing.needs_ohlc();
    if needs_ohlc && (high.len() < n || low.len() < n) {
        return Err(crate::error::AppError::Msg(format!(
            "配置了止损/ATR 仓位但 high/low 长度不足: high={}, low={}, close={n}",
            high.len(),
            low.len()
        )));
    }
    if n < 2 {
        // 无成交 ⇒ 权益就是初始资金; 执行口径仍需回填, 否则报告里同一列
        // 会出现"数据不足的行是 0、正常行是真实值"的混排。
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(summary);
    }
    // collect_curves = false: 不物化曲线与逐笔 Trade; 曲线统计在线累计,
    // 逐笔 pnl 仍保留(O(交易笔数))用于高阶矩与分半 PF。
    let out = scan_trades(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        &[],
        initial_cash,
        exec,
        risk,
        sizing,
        false,
    )?;
    debug_assert!(
        !out.collect_curves && out.detail.trades.is_empty(),
        "simulate 不得物化落盘产物"
    );
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);
    Ok(summary)
}

/// 带策略止损线的盘中回测(默认全仓)。
///
/// 保留旧签名供既有调用/测试使用; 需要仓位模式时用
/// [`simulate_ohlc_with_stops_and_sizing`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_ohlc_with_stops(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SummaryMetrics> {
    simulate_ohlc_with_stops_and_sizing(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        initial_cash,
        bars_per_year,
        exec,
        risk,
        &SizingConfig::AllIn,
    )
}

/// 执行一次回测并额外产出逐笔交易与两条曲线。
///
/// 指标经与 [`simulate`] **同一条**路径计算, 二者逐位一致。
pub fn simulate_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc(
        time,
        close,
        close,
        close,
        open,
        signals,
        initial_cash,
        bars_per_year,
        exec,
        &StopConfig::default(),
    )
}

/// 带盘中 high/low 与持仓期止损的详细回测。
///
/// 与 [`simulate_ohlc`] 使用**同一条**指标计算路径, 仅额外物化逐笔交易与曲线。
///
/// # Errors
/// 同 [`simulate_ohlc`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc_with_stops(
        time,
        high,
        low,
        close,
        open,
        signals,
        None,
        initial_cash,
        bars_per_year,
        exec,
        risk,
    )
}

/// 带策略止损线的详细回测。
///
/// 与 [`simulate_detailed_ohlc`] 相同, 额外接受策略输出的 [`StopLines`]。
///
/// # Errors
/// 同 [`simulate_ohlc_with_stops`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc_with_stops_and_sizing(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
) -> crate::error::Result<SimResult> {
    exec.validate_rates()?;
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let needs_ohlc =
        !risk.is_none() || strategy_stops.is_some_and(|s| !s.is_none()) || sizing.needs_ohlc();
    if needs_ohlc && (high.len() < n || low.len() < n) {
        return Err(crate::error::AppError::Msg(format!(
            "配置了止损/ATR 仓位但 high/low 长度不足: high={}, low={}, close={n}",
            high.len(),
            low.len()
        )));
    }
    if n < 2 {
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(SimResult {
            summary,
            trades: Vec::new(),
            equity_curve: Vec::new(),
            fixed_curve: Vec::new(),
        });
    }
    // detailed 路径要落盘, 故额外物化逐笔 Trade 与两条曲线。
    // 注意指标仍只经 `out.metrics` 计算 —— 与 `simulate` 完全同源。
    let out = scan_trades(
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        time,
        initial_cash,
        exec,
        risk,
        sizing,
        true,
    )?;
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);

    let detail = out.detail;
    let equity_curve: Vec<(f64, f64)> = detail
        .equity
        .into_iter()
        .enumerate()
        .map(|(i, eq)| {
            let ts = if i < time.len() {
                time[i] as f64
            } else {
                i as f64
            };
            (ts, eq)
        })
        .collect();

    Ok(SimResult {
        summary,
        trades: detail.trades,
        equity_curve,
        fixed_curve: detail.fixed,
    })
}

/// 带策略止损线的详细回测(默认全仓)。
///
/// 保留旧签名供既有调用/测试使用; 需要仓位模式时用
/// [`simulate_detailed_ohlc_with_stops_and_sizing`]。
#[allow(clippy::too_many_arguments)]
pub fn simulate_detailed_ohlc_with_stops(
    time: &[i64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
    risk: &StopConfig,
) -> crate::error::Result<SimResult> {
    simulate_detailed_ohlc_with_stops_and_sizing(
        time,
        high,
        low,
        close,
        open,
        signals,
        strategy_stops,
        initial_cash,
        bars_per_year,
        exec,
        risk,
        &SizingConfig::AllIn,
    )
}

/// 批量回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch(
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SummaryMetrics>> {
    signal_batch
        .par_iter()
        .map(|signals| simulate(close, open, signals, initial_cash, bars_per_year, exec))
        .collect::<crate::error::Result<Vec<_>>>()
}

/// 批量详细回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SimResult>> {
    signal_batch
        .par_iter()
        .map(|signals| {
            simulate_detailed(
                time,
                close,
                open,
                signals,
                initial_cash,
                bars_per_year,
                exec,
            )
        })
        .collect::<crate::error::Result<Vec<_>>>()
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    fn make_prices(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 20.0)
            .collect()
    }

    /// 固定 1 万初始资金、无成本、无止损的详细回测, 仅改变仓位模式。
    fn sized_detail(
        high: &[f64],
        low: &[f64],
        close: &[f64],
        open: &[f64],
        signals: &[Signal],
        sizing: SizingConfig,
    ) -> SimResult {
        simulate_detailed_ohlc_with_stops_and_sizing(
            &[],
            high,
            low,
            close,
            open,
            signals,
            None,
            crate::money::Amount::from_i64(10_000),
            365.0,
            &ExecConfig::default(),
            &StopConfig::default(),
            &sizing,
        )
        .expect("合法回测必须成功")
    }

    #[test]
    fn test_empty_returns_default() {
        let m = simulate(
            &[],
            &[],
            &[],
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("空输入必须成功返回默认值");
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_no_signals_hold_value() {
        let close = vec![100.0; 100];
        let sig = vec![Signal::Hold; 100];
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert!((m.total_return_pct).abs() < 1e-10);
    }

    #[test]
    fn test_buy_and_hold() {
        let close: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        let mut sig = vec![Signal::Hold; 100];
        sig[0] = Signal::Buy;
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert!(m.total_return_pct > 95.0);
        assert_eq!(m.num_trades, 1);
    }

    #[test]
    fn test_default_exec_fills_at_next_bar_open() {
        // 默认口径 = delay 1 + fill bar 开盘价: bar0 信号在 bar1 开盘成交,
        // 末根强制收盘平仓。`delay_bars` 恒 >= 1(NonZeroUsize), "当根成交"分支
        // 已从引擎层移除, 故不存在与之逐位一致的旧口径可比。
        let close = vec![100.0, 102.0, 101.0, 105.0];
        let open = vec![99.0, 100.5, 102.5, 101.5];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 1);
        assert_eq!(m.delay_bars, 1);
        // 进 open[1]=100.5, 出 close[3]=105; 名义 1000/100.5。
        let qty = 1000.0 / 100.5;
        let expect = (105.0 - 100.5) * qty;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
    }

    /// delay_bars = 0(信号当根成交, 前视乐观)已从**引擎层**彻底移除:
    /// 字段是 `NonZeroUsize`, 类型上无法构造, 构造期校验不可能被绕过。
    #[test]
    fn zero_delay_is_rejected_at_every_entry_point() {
        assert_eq!(NonZeroUsize::MIN.get(), 1, "缺省必须是 1 bar");
        assert!(ExecConfig::default().delay() >= 1);
        assert!(ExecConfig::try_from_bps_fill(0, 0.0, 0.0, false).is_err());
        for (d, fee, slip, close) in [
            (0, 0.0, 0.0, false),
            (0, 2.0, 1.0, true),
            (0, -1.0, 0.0, false),
        ] {
            assert!(
                ExecConfig::try_from_bps_fill(d, fee, slip, close).is_err(),
                "delay=0 必须被拒(d={d}, fee={fee}, slip={slip})"
            );
        }
        // from_bps / from_bps_fill 是 try_from 的 infallible 包装, 遇 0 直接 panic。
        let panicked = std::panic::catch_unwind(|| ExecConfig::from_bps(0, 0.0, 0.0)).is_err();
        assert!(panicked, "from_bps(0) 应 panic");
    }

    /// **入口契约**: 负/非有限费率与滑点必须在 `ExecConfig` 构造期拒绝。
    ///
    /// 配置路径已有 newtype 保证, 但 `ExecConfig` 是公开引擎 API; 负费率会让
    /// 每笔交易"倒赚", `NaN` 会沿权益曲线污染全部指标。两者都不应等到回测
    /// 结束才以 `NaN` 的形式暴露。
    #[test]
    fn exec_rejects_negative_or_nonfinite_costs() {
        for (fee, slip) in [
            (-1.0, 0.0),
            (0.0, -1.0),
            (f64::NAN, 0.0),
            (0.0, f64::NAN),
            (f64::INFINITY, 0.0),
            (0.0, f64::INFINITY),
            (f64::NEG_INFINITY, 0.0),
        ] {
            assert!(
                ExecConfig::try_from_bps_fill(1, fee, slip, false).is_err(),
                "fee={fee}, slip={slip} 必须被拒"
            );
        }
        let ok = ExecConfig::try_from_bps_fill(1, 2.0, 1.0, true).expect("合法成本应通过");
        assert!((ok.fee_rate - 0.0002).abs() < 1e-15);
        assert!((ok.slip_rate - 0.0001).abs() < 1e-15);
        let panicked = std::panic::catch_unwind(|| ExecConfig::from_bps(1, -1.0, 0.0)).is_err();
        assert!(panicked, "from_bps 遇负费率应 panic(调用方 bug)");
    }

    /// 公开字段仍可被直接构造; 引擎入口必须再校验一次, 不能只依赖构造函数。
    #[test]
    fn engine_rejects_invalid_exec_fields_even_without_constructor() {
        let prices = [100.0, 101.0, 102.0];
        let bad = ExecConfig {
            fee_rate: -0.01,
            ..ExecConfig::default()
        };
        let err = simulate(
            &prices,
            &prices,
            &[],
            crate::money::Amount::from_i64(1000),
            252.0,
            &bad,
        )
        .expect_err("负费率必须在引擎入口被拒");
        assert!(format!("{err}").contains("fee_rate"), "{err}");
        let bad_nan = ExecConfig {
            slip_rate: f64::NAN,
            ..ExecConfig::default()
        };
        assert!(
            simulate(
                &prices,
                &prices,
                &[],
                crate::money::Amount::from_i64(1000),
                252.0,
                &bad_nan,
            )
            .is_err(),
            "NaN 滑点必须在引擎入口被拒"
        );
        let bad_huge = ExecConfig {
            fee_rate: f64::MAX,
            ..ExecConfig::default()
        };
        assert!(
            simulate(
                &prices,
                &prices,
                &[],
                crate::money::Amount::from_i64(1000),
                252.0,
                &bad_huge,
            )
            .is_err(),
            "换算回 bps 会溢出的费率必须在引擎入口被拒"
        );
    }

    #[test]
    fn test_delay_one_fills_at_next_open() {
        // delay=1: bar0 多单按 open[1] 成交; bar2 反手空按 open[3] 离场并建仓。
        let close = vec![100.0, 102.0, 101.0, 105.0, 107.0];
        let open = vec![99.0, 101.0, 102.5, 100.5, 106.0];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps(1, 0.0, 0.0);
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 2);
        // 多头: 进 101.0 出 100.5 → (100.5-101.0)/101.0*1000。
        let expect_long = (100.5 - 101.0) / 101.0 * 1000.0;
        // 空头: 进 100.5, 末根 107 收盘离场 → (100.5-107)/100.5*1000。
        let expect_short = (100.5 - 107.0) / 100.5 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - (expect_long + expect_short)).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
        assert_eq!(m.delay_bars, 1);
    }

    #[test]
    fn test_fill_close_uses_next_bar_close() {
        // delay=1 + fill_close: bar0 多单按 close[1] 成交; bar2 反手空按 close[3] 离场并建仓。
        // 数据与 test_delay_one_fills_at_next_open 同构, 仅 fill 价位 open→close。
        let close = vec![100.0, 102.0, 101.0, 105.0, 107.0];
        let open = vec![99.0, 101.0, 102.5, 100.5, 106.0];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps_fill(1, 0.0, 0.0, true);
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 2);
        // 多头: 进 102.0 出 105.0 → (105.0-102.0)/102.0*1000。
        let expect_long = (105.0 - 102.0) / 102.0 * 1000.0;
        // 空头: 进 105.0, 末根 107 收盘离场 → (105.0-107)/105.0*1000。
        let expect_short = (105.0 - 107.0) / 105.0 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - (expect_long + expect_short)).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
    }

    /// 权益曲线必须只反映**已成交**持仓: 信号 bar 不能用未来 fill bar 的价格盯市。
    ///
    /// 旧实现在 bar0 收到 Buy 就立刻把 `cur_pos`/`cash` 改成长仓, 却用
    /// `open[1] = 200` 作为成交价: `cash = 0`、`qty = 5`, 于是 bar0 权益
    /// `= qty × close[0] = 500` —— 在**尚未成交**的 bar 上凭空产生 50% 亏损,
    /// 整条曲线与 Sharpe/回撤全部失真。
    #[test]
    fn equity_curve_does_not_mark_to_future_fill_price() {
        let close = vec![100.0, 200.0, 200.0];
        let open = vec![100.0, 200.0, 200.0];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold];
        let d = simulate_detailed(
            &[0, 1, 2],
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        // bar0 尚未成交(初始资金); bar1 开盘成交后按 close[1] 盯市; bar2 收盘离场。
        for (i, (_, eq)) in d.equity_curve.iter().enumerate() {
            assert!(
                (eq - 1000.0).abs() < 1e-9,
                "bar{i} 权益 {eq} 应为 1000(成交前不得按未来价盯市)"
            );
        }
        // 零收益往返: 波动为 0, Sharpe 必须是 0 而不是被伪波动放大到 11.22。
        assert_eq!(d.summary.sharpe_ratio, 0.0);
        assert_eq!(d.summary.max_drawdown_pct, 0.0);
        assert_eq!(d.trades.len(), 1);
        assert!((d.trades[0].entry_price - 200.0).abs() < 1e-12);
    }

    /// `delay_bars = 2`: 信号 bar0 的目标方向要到 bar2 才成交; bar1 权益必须仍是
    /// 初始资金, 交易的实际成交 bar 也必须是 fill bar(而非信号 bar)。
    #[test]
    fn delay_two_defers_position_until_the_second_fill_bar() {
        let close = vec![100.0, 100.0, 100.0, 120.0, 120.0];
        let open = close.clone();
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = simulate_detailed(
            &[0, 1, 2, 3, 4],
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::from_bps(2, 0.0, 0.0),
        )
        .expect("合法回测必须成功");
        assert_eq!(d.equity_curve[0].1, 1000.0, "bar0 未成交");
        assert_eq!(d.equity_curve[1].1, 1000.0, "bar1 仍未成交");
        assert_eq!(d.equity_curve[3].1, 1200.0, "bar3 应按 close=120 盯市");
        assert_eq!(d.trades.len(), 2);
        // 多头: bar2 开 100 -> bar4 开 120; 持仓 2 bar。
        assert!((d.trades[0].entry_price - 100.0).abs() < 1e-12);
        assert!((d.trades[0].exit_price - 120.0).abs() < 1e-12);
        assert_eq!(d.trades[0].bars_held, 2);
        assert_eq!(d.trades[0].entry_time, 2);
        assert_eq!(d.trades[0].exit_time, 4);
    }

    /// 逐笔 `pnl` 必须包含**开仓**佣金: `sum(trade.pnl) == final_equity - initial`。
    ///
    /// 旧实现只扣平仓佣金, 逐笔 pnl 系统性偏乐观, win/loss/PF/SQN 全部被污染。
    #[test]
    fn trade_pnl_sums_to_equity_change_with_both_fees() {
        let close = vec![
            100.0, 104.0, 103.0, 109.0, 107.0, 111.0, 110.0, 114.0, 112.0,
        ];
        let open = vec![
            100.0, 101.0, 105.0, 102.0, 110.0, 106.0, 112.0, 109.0, 115.0,
        ];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
        ];
        let initial = crate::money::Amount::from_i64(10_000);
        let exec = ExecConfig::from_bps(1, 10.0, 5.0);
        let d = simulate_detailed(&[], &close, &open, &sig, initial, 252.0, &exec)
            .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 3, "应覆盖多/空/末尾强平三条路径");
        let mut sum = crate::money::Amount::ZERO;
        for t in &d.trades {
            sum = sum.plus(t.pnl).expect("金额累加不得溢出");
        }
        let delta = d
            .summary
            .final_equity
            .minus(initial)
            .expect("金额相减不得溢出");
        let diff = sum.minus(delta).expect("金额相减不得溢出").to_f64().abs();
        assert!(
            diff < 1e-6,
            "sum(trade.pnl)={sum:?} 与 final-initial={delta:?} 不一致(diff={diff})"
        );
    }

    /// 偏度/峰度必须匹配标准小样本校正公式(pandas `skew()` / `kurt()` 口径)。
    ///
    /// 样本 `[1,2,3,4,10]`: 校正偏度 = 1.6970562748477143, 超额峰度 = 3.152。
    /// 旧实现偏度小 `((n-1)/n)^1.5`、峰度第一项多乘 `n`。
    #[test]
    fn skewness_and_kurtosis_match_corrected_sample_moments() {
        let acc = TradeAccum {
            num_trades: 5,
            pnls: vec![1.0, 2.0, 3.0, 4.0, 10.0],
            ..TradeAccum::default()
        };
        let mut eq = CurveAccum::new(1000.0);
        eq.push(1000.0);
        eq.push(1000.0);
        let mut fx = CurveAccum::new(1000.0);
        fx.push(1000.0);
        fx.push(1000.0);
        let m = compute_metrics(
            &MetricsSource {
                acc,
                eq,
                fx,
                final_equity: crate::money::Amount::from_i64(1000),
            },
            crate::money::Amount::from_i64(1000),
            252.0,
        )
        .expect("指标计算必须成功");
        assert!(
            (m.skewness - 1.697_056_274_847_714_3).abs() < 1e-12,
            "skewness={}",
            m.skewness
        );
        assert!(
            (m.kurtosis - 3.152).abs() < 1e-12,
            "kurtosis={}",
            m.kurtosis
        );
    }

    #[test]
    fn test_buy_hold_first_close_to_last_close() {
        // 100→110 名义 1000 → +100/+0.1; 不足 2 根或首价非法 → 0。
        let (pnl, ret) = buy_hold_pnl(&[100.0, 105.0, 110.0], crate::money::Amount::from_i64(1000))
            .expect("合法基准必须成功");
        assert!((pnl.to_f64() - 100.0).abs() < 1e-9, "{pnl}");
        assert!((ret - 0.1).abs() < 1e-12, "{ret}");
        assert_eq!(
            buy_hold_pnl(&[100.0], crate::money::Amount::from_i64(1000))
                .expect("数据不足应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
        assert_eq!(
            buy_hold_pnl(&[], crate::money::Amount::from_i64(1000))
                .expect("非法输入应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
        assert_eq!(
            buy_hold_pnl(&[0.0, 110.0], crate::money::Amount::from_i64(1000))
                .expect("非法输入应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
    }

    #[test]
    fn test_fee_and_slippage_deducted_per_side() {
        // fee 10bp + slip 10bp 双边: 进出各扣一次。用 Close 离场以隔离单笔数学。
        // delay 恒 >= 1: bar0 信号在 bar1 成交, bar1 的 Close 在 bar2 成交。
        let close = vec![100.0, 100.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Close, Signal::Hold];
        let exec = ExecConfig::from_bps(1, 10.0, 10.0);
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 1);
        // 进 open[1]*1.001 = 100.1, 出 open[2]*0.999 = 109.89; 名义 1000/100.1;
        // 毛利 (109.89-100.1)*qty - 进出佣金 (100.1+109.89)*qty*0.001。
        let qty = 1000.0 / 100.1;
        let expect = (109.89 - 100.1) * qty - (100.1 + 109.89) * qty * 0.001;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
        assert!((m.fee_bps - 10.0).abs() < 1e-12);
        assert!((m.slippage_bps - 10.0).abs() < 1e-12);
    }

    #[test]
    fn test_trailing_open_beyond_data_skipped() {
        // 末根信号 delay=1 无未来可成交: 不开仓, 交易数为 0。
        let close = vec![100.0, 101.0, 102.0];
        let open = vec![99.0, 100.0, 101.0];
        let sig = vec![Signal::Hold, Signal::Hold, Signal::Buy];
        let exec = ExecConfig::from_bps(1, 0.0, 0.0);
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_fixed_leg_matches_hand_calc() {
        // fixed 腿: 单多 100→110, 名义 1000 → +100, 期望 +1000bp。
        // delay 恒 >= 1: bar0 进(填在 bar1), bar2 的 Close 填在 bar3。
        let close = vec![100.0, 100.0, 110.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Close, Signal::Hold];
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 1);
        assert!(
            (m.fixed_total_return - 0.1).abs() < 1e-12,
            "{}",
            m.fixed_total_return
        );
        assert!(
            (m.fixed_expectancy_bps - 1000.0).abs() < 1e-9,
            "{}",
            m.fixed_expectancy_bps
        );
        assert!(
            (m.fixed_profit_factor - 100.0).abs() < 1e-9,
            "{}",
            m.fixed_profit_factor
        );
        assert!(m.fixed_sharpe_ratio > 0.0);
    }

    /// 固定仓位: 复利腿随当前现金增长, fixed 腿始终按初始资金计价。
    #[test]
    fn fixed_sizing_compounds_main_leg_but_not_fixed_leg() {
        // 两笔各 +10%: AllIn 10000→12100; 50% 仓位 10000→11025;
        // fixed 腿每笔名义 5000, 两笔各 +500 → 11000。
        let close = vec![100.0, 100.0, 110.0, 110.0, 110.0, 121.0, 121.0];
        let open = close.clone();
        let mut sig = vec![Signal::Hold; close.len()];
        sig[0] = Signal::Buy;
        sig[2] = Signal::Close;
        sig[3] = Signal::Buy;
        sig[5] = Signal::Close;
        let out = sized_detail(
            &close,
            &close,
            &close,
            &open,
            &sig,
            SizingConfig::Fixed { fraction_pct: 50.0 },
        );
        assert_eq!(out.summary.num_trades, 2);
        assert!(
            (out.summary.final_equity.to_f64() - 11_025.0).abs() < 1e-9,
            "{}",
            out.summary.final_equity
        );
        assert!(
            (out.summary.total_fixed_pnl.to_f64() - 1_000.0).abs() < 1e-9,
            "{}",
            out.summary.total_fixed_pnl
        );
        let fixed_last = *out.fixed_curve.last().expect("曲线非空");
        assert!((fixed_last - 11_000.0).abs() < 1e-9, "{fixed_last}");
    }

    /// ATR 仓位: 名义 = 风险预算 / (mult × ATR); 无杠杆模型下截断到 1 倍权益。
    #[test]
    fn atr_sizing_sizes_by_risk_and_caps_at_no_leverage() {
        // 恒定 TR=5 ⇒ ATR(2)=5。risk 1%、2×ATR ⇒ 名义 10% ⇒ 10 张。
        let close = vec![100.0; 8];
        let high = vec![102.5; 8];
        let low = vec![97.5; 8];
        let open = close.clone();
        let mut sig = vec![Signal::Hold; 8];
        sig[2] = Signal::Buy;
        sig[4] = Signal::Close;
        let out = sized_detail(
            &high,
            &low,
            &close,
            &open,
            &sig,
            SizingConfig::Atr {
                risk_pct: 1.0,
                period: 2,
                mult: 2.0,
                max_position_pct: 100.0,
                fallback_fraction_pct: 0.0,
            },
        );
        assert_eq!(out.trades.len(), 1);
        assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(10));

        // risk 100%、1×ATR ⇒ 名义 20 倍权益, 但引擎无杠杆, 截断到 1 倍。
        let out = sized_detail(
            &high,
            &low,
            &close,
            &open,
            &sig,
            SizingConfig::Atr {
                risk_pct: 100.0,
                period: 2,
                mult: 1.0,
                max_position_pct: 100.0,
                fallback_fraction_pct: 0.0,
            },
        );
        assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(100));
    }

    /// ATR 未就绪时按 fallback 比例开仓; fallback=0 则继续空仓等待。
    #[test]
    fn atr_sizing_uses_fallback_until_atr_is_ready() {
        let close = vec![100.0; 8];
        let high = vec![102.5; 8];
        let low = vec![97.5; 8];
        let open = close.clone();
        let mut sig = vec![Signal::Hold; 8];
        sig[1] = Signal::Buy;
        sig[3] = Signal::Close;
        let out = sized_detail(
            &high,
            &low,
            &close,
            &open,
            &sig,
            SizingConfig::Atr {
                risk_pct: 1.0,
                period: 10,
                mult: 2.0,
                max_position_pct: 100.0,
                fallback_fraction_pct: 20.0,
            },
        );
        assert_eq!(out.trades.len(), 1);
        assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(20));
    }

    /// Kelly: 样本不足走 fallback; 样本足够后按最近 N 笔的胜率/赔率折减。
    #[test]
    fn kelly_uses_recent_trade_stats_after_min_trades() {
        let close = vec![
            100.0, 100.0, 110.0, 110.0, 110.0, 104.5, 104.5, 104.5, 104.5, 104.5,
        ];
        let open = close.clone();
        let mut sig = vec![Signal::Hold; close.len()];
        sig[0] = Signal::Buy;
        sig[2] = Signal::Close;
        sig[3] = Signal::Buy;
        sig[5] = Signal::Close;
        sig[6] = Signal::Buy;
        sig[8] = Signal::Close;
        let out = sized_detail(
            &close,
            &close,
            &close,
            &open,
            &sig,
            SizingConfig::Kelly {
                lookback: 2,
                min_trades: 2,
                kelly_fraction: 0.5,
                max_position_pct: 100.0,
                fallback_fraction_pct: 100.0,
            },
        );
        assert_eq!(out.trades.len(), 3);
        // 前两笔 fallback 全仓: 10000→11000→10450; 第三笔半 Kelly:
        // p=0.5, b=2 ⇒ f=0.25, 半 Kelly=0.125 ⇒ 10450×0.125/104.5 = 12.5 张。
        assert_eq!(
            out.trades[2].qty,
            crate::money::Amount::from_f64(12.5).expect("12.5 合法")
        );
    }

    /// Kelly 估计出非正优势时不开仓(而不是回退到 fallback)。
    #[test]
    fn kelly_with_non_positive_edge_skips_entry() {
        let close = vec![
            100.0, 100.0, 110.0, 110.0, 110.0, 99.0, 99.0, 99.0, 99.0, 99.0,
        ];
        let open = close.clone();
        let mut sig = vec![Signal::Hold; close.len()];
        sig[0] = Signal::Buy;
        sig[2] = Signal::Close;
        sig[3] = Signal::Buy;
        sig[5] = Signal::Close;
        sig[6] = Signal::Buy;
        sig[8] = Signal::Close;
        let out = sized_detail(
            &close,
            &close,
            &close,
            &open,
            &sig,
            SizingConfig::Kelly {
                lookback: 2,
                min_trades: 2,
                kelly_fraction: 0.5,
                max_position_pct: 100.0,
                fallback_fraction_pct: 100.0,
            },
        );
        assert_eq!(out.summary.num_trades, 2, "f<=0 时第三笔不得开仓");
    }

    /// 平仓必须复用开仓时存的 fixed 腿数量, 不能按 `initial_cash` 重算。
    #[test]
    fn close_outcome_uses_stored_fixed_qty() {
        let out = close_outcome(CloseArgs {
            cur_pos: Side::Long,
            pos_qty: crate::money::Amount::from_i64(10),
            pos_entry: 100.0,
            pos_entry_time: 0,
            pos_entry_bar: 0,
            exit_price: 110.0,
            exit_time: 1,
            exit_bar: 1,
            // 故意与复利腿不同: 若平仓时按 initial_cash 重算, 这里会得到 100 而非 250。
            fixed_qty: crate::money::Amount::from_i64(25),
            trade_mfe: 0.0,
            trade_mae: 0.0,
            fee_rate: 0.0,
            reason: ExitReason::Signal,
        })
        .expect("合法平仓");
        assert_eq!(out.pnl, crate::money::Amount::from_i64(100));
        assert_eq!(out.fixed_pnl, crate::money::Amount::from_i64(250));
    }

    /// `params` 只能有一个 `;` 分隔符: `params_to_json` 只按首个 `;` 切分。
    #[test]
    fn apply_to_appends_stop_and_sizing_after_one_separator() {
        let base = "period=20";
        let no_stop = StopConfig::default();
        assert_eq!(no_stop.apply_to(base, &SizingConfig::AllIn), base);
        assert_eq!(
            no_stop.apply_to(base, &SizingConfig::Fixed { fraction_pct: 20.0 }),
            "period=20;size=fixed,size_fraction_pct=20"
        );

        let stop = StopConfig {
            stop_loss_pct: Some(2.0),
            ..StopConfig::default()
        };
        assert_eq!(
            stop.apply_to(base, &SizingConfig::AllIn),
            "period=20;stop_loss_pct=2"
        );
        assert_eq!(
            stop.apply_to(base, &SizingConfig::Fixed { fraction_pct: 20.0 }),
            "period=20;stop_loss_pct=2,size=fixed,size_fraction_pct=20"
        );
        for sizing in [
            SizingConfig::AllIn,
            SizingConfig::Fixed { fraction_pct: 20.0 },
        ] {
            assert_eq!(
                stop.apply_to(base, &sizing).matches(';').count(),
                1,
                "stop+sizing 只能有一个分隔符"
            );
        }
    }

    #[test]
    fn test_turnover_per_year_counts_round_trips_annualized() {
        let close = make_prices(200);
        let mut sig = vec![Signal::Hold; 200];
        sig[0] = Signal::Buy;
        sig[50] = Signal::Sell;
        sig[100] = Signal::Buy;
        sig[150] = Signal::Sell;

        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 4);
        // 年数 = 199 根 bar / 252 bar 每年
        let expected = 4.0 / (199.0 / 252.0);
        assert!(
            (m.turnover_per_year - expected).abs() < 1e-9,
            "{}",
            m.turnover_per_year
        );
    }

    #[test]
    fn test_turnover_per_year_is_zero_without_trades() {
        let close = make_prices(200);
        let m = simulate(
            &close,
            &close,
            &[Signal::Hold; 200],
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert_eq!(m.turnover_per_year, 0.0);
    }

    #[test]
    fn test_simulate_batch_matches_single() {
        let close = make_prices(200);
        let sig1 = vec![Signal::Hold; 200];
        let mut sig2 = vec![Signal::Hold; 200];
        sig2[0] = Signal::Buy;

        let batch = vec![sig1.clone(), sig2.clone()];
        let exec = ExecConfig::default();
        let results = simulate_batch(
            &close,
            &close,
            &batch,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法批量回测必须成功");
        let r1 = simulate(
            &close,
            &close,
            &sig1,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        let r2 = simulate(
            &close,
            &close,
            &sig2,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], r1);
        assert_eq!(results[1], r2);
    }

    /// [`CurveAccum`] 与「物化曲线后单遍遍历」的**逐位一致**证明。
    ///
    /// 覆盖 5 类曲线形态, 每类同时锁定 len / last / 全部 5 个统计量:
    /// - 单调上涨: 无回撤、下行样本为 0(sortino 分母退化)
    /// - 锯齿: 有回撤、有下行样本(sortino 与 sharpe 分母不同 → 检验 ddof 约定)
    /// - 首根即非初始值: 检验「首点不参与 peak/max_dd」的口径
    /// - 穿零(空头浮空): 检验 [`log_ret`] 的定义域保护, 且 sortino 有限非 NaN
    /// - 含 NaN: 检验守卫退化为 0 而非污染 NaN
    #[test]
    fn test_curve_accum_is_bit_identical_to_materialized_reference() {
        let cases: Vec<(&str, Vec<f64>)> = vec![
            ("单调上涨", (0..64).map(|i| 1000.0 + i as f64).collect()),
            (
                "锯齿",
                (0..64)
                    .map(|i| {
                        if i % 2 == 0 {
                            1000.0 + i as f64
                        } else {
                            900.0 - i as f64
                        }
                    })
                    .collect(),
            ),
            ("首根即持仓(首点 != initial)", {
                let mut v = vec![1234.5];
                v.extend((1..64).map(|i| 1000.0 + i as f64));
                v
            }),
            (
                "穿零(空头浮空)",
                (0..64).map(|i| 1000.0 - i as f64 * 40.0).collect(),
            ),
            ("含 NaN", {
                let mut v: Vec<f64> = (0..64)
                    .map(|i| 1000.0 + (i as f64 * 3.7).sin() * 50.0)
                    .collect();
                v[10] = f64::NAN;
                v
            }),
        ];

        for (name, curve) in cases {
            let initial = curve[0];
            let mut acc = CurveAccum::new(initial);
            for &eq in &curve {
                acc.push(eq);
            }
            let got = acc.stats(365.0);
            let want = reference_curve_stats(&curve, initial, 365.0);

            assert_eq!(acc.len(), curve.len(), "{name}: len");
            assert_eq!(
                acc.last().to_bits(),
                curve[curve.len() - 1].to_bits(),
                "{name}: last"
            );
            assert_eq!(
                got.sharpe.to_bits(),
                want.sharpe.to_bits(),
                "{name}: sharpe"
            );
            assert_eq!(
                got.sortino.to_bits(),
                want.sortino.to_bits(),
                "{name}: sortino"
            );
            assert_eq!(
                got.max_dd.to_bits(),
                want.max_dd.to_bits(),
                "{name}: max_dd"
            );
            assert_eq!(
                got.volatility.to_bits(),
                want.volatility.to_bits(),
                "{name}: volatility"
            );
            assert_eq!(
                got.total_return.to_bits(),
                want.total_return.to_bits(),
                "{name}: total_return"
            );
            for (k, v) in [
                ("sharpe", got.sharpe),
                ("sortino", got.sortino),
                ("max_dd", got.max_dd),
                ("volatility", got.volatility),
                ("total_return", got.total_return),
            ] {
                assert!(v.is_finite(), "{name}: {k} = {v} 非有限");
            }
        }
    }

    /// [`simulate`] 必须**不物化**落盘产物(曲线/Trade 均不保留); 这是 O2 的全部意义。
    ///
    /// 允许保留 O(交易笔数) 的 pnl 统计序列 —— 它远小于 O(T) 的双曲线,
    /// 且是 SQN/偏度/峰度/分半 PF 的必需输入。
    #[test]
    fn test_simulate_does_not_materialize_equity_curve() {
        let close = make_prices(100_000);
        let mut sig = vec![Signal::Hold; 100_000];
        sig[0] = Signal::Buy;
        let exec = ExecConfig::default();
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            365.0,
            &exec,
        )
        .expect("合法回测必须成功");
        let d = simulate_detailed(
            &[],
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            365.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(d.equity_curve.len(), 100_000, "detailed 必须物化曲线");
        assert_eq!(d.fixed_curve.len(), 100_000);

        // Q2 结构性保证: `scan_trades` 在 collect_curves=false 时不得产出任何
        // DetailArtifacts(逐笔 Trade + 两条曲线)。类型上由 Option 表达, 此处
        // 在 debug 构建下由 debug_assert! 复核。
        let out = scan_trades(
            &close,
            &close,
            &close,
            &close,
            &sig,
            None,
            &[],
            crate::money::Amount::from_i64(10_000),
            &exec,
            &StopConfig::default(),
            &SizingConfig::default(),
            false,
        )
        .expect("合法回测必须成功");
        assert!(
            !out.collect_curves && out.detail.trades.is_empty(),
            "非 detailed 路径不得物化落盘产物"
        );
        for f in [
            m.sharpe_ratio,
            m.sortino_ratio,
            m.max_drawdown_pct,
            m.volatility,
            m.fixed_sharpe_ratio,
            m.fixed_max_drawdown_pct,
        ] {
            assert!(f.is_finite(), "指标非有限: {f}");
        }
        assert_eq!(m.sharpe_ratio.to_bits(), d.summary.sharpe_ratio.to_bits());
        assert_eq!(m.sortino_ratio.to_bits(), d.summary.sortino_ratio.to_bits());
        assert_eq!(
            m.max_drawdown_pct.to_bits(),
            d.summary.max_drawdown_pct.to_bits()
        );
        assert_eq!(m.volatility.to_bits(), d.summary.volatility.to_bits());
        assert_eq!(
            m.fixed_sharpe_ratio.to_bits(),
            d.summary.fixed_sharpe_ratio.to_bits()
        );
    }

    /// 空头浮空(权益穿零)不得污染统计量: 不得出现 NaN, volatility 不得静默归零。
    #[test]
    fn test_short_blowup_yields_finite_nonzero_ratios() {
        let n = 200;
        let close: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 1.5).collect();
        let mut sig = vec![Signal::Hold; n];
        sig[0] = Signal::Sell; // 全程空头, 权益必然穿零
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            8760.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert!(
            m.final_equity.to_f64() < 0.0,
            "该用例应确实穿零: {}",
            m.final_equity
        );
        for (k, v) in [
            ("sharpe", m.sharpe_ratio),
            ("sortino", m.sortino_ratio),
            ("volatility", m.volatility),
            ("max_dd", m.max_drawdown_pct),
        ] {
            assert!(v.is_finite(), "{k} = {v} 非有限(权益穿零被污染)");
        }
        assert!(
            m.volatility > 0.0,
            "volatility 被静默归零(比 NaN 更危险: 会被读成零波动低风险): {}",
            m.volatility
        );
    }

    /// sortino 的分母必须是**下行 bar 数**(ddof=1), 与 sharpe 同自由度。
    /// 若分母误用全部 bar 数, "下跌集中在少数 bar"的曲线会得到明显更小的 sortino。
    #[test]
    fn test_sortino_uses_downside_count_as_denominator() {
        let mut curve = vec![1000.0f64];
        for i in 0..10 {
            curve.push(1000.0 * 1.01f64.powi(i + 1));
        }
        curve.push(900.0);
        curve.push(800.0);
        let mut acc = CurveAccum::new(curve[0]);
        for &c in &curve {
            acc.push(c);
        }
        let st = acc.stats(1.0);

        let rets: Vec<f64> = bar_log_returns(&curve).collect();
        let down_sq: f64 = rets.iter().filter(|&&r| r < 0.0).map(|r| r * r).sum();
        let n_down = rets.iter().filter(|&&r| r < 0.0).count() as f64;
        let mut mean = 0.0;
        for (i, r) in rets.iter().enumerate() {
            mean += (r - mean) / (i + 1) as f64;
        }
        let expect = mean / (down_sq / (n_down - 1.0)).sqrt();
        assert!(
            (st.sortino - expect).abs() < 1e-12,
            "sortino {}(应={expect}) 分母用了错误的 bar 数",
            st.sortino
        );
        let wrong = mean / (down_sq / rets.len() as f64).sqrt();
        assert!(
            (st.sortino - wrong).abs() > 1e-6,
            "两种口径碰巧相同, 该用例失去区分力"
        );
    }

    /// SQN = √n · mean/stdev (Van Thorp), **不封顶**; `√n` 是定义中的统计显著性
    /// 因子, 封顶会抹掉"证据量"这一维度。小样本可读性由 `num_trades >= 30`
    /// 门槛解决(见 `sqn` 字段文档)。
    #[test]
    fn test_sqn_scales_with_full_sample_size() {
        // 每笔的**相对**收益固定按 [+r, +r, -r] 循环, 故 mean/stdev 与笔数无关,
        // SQN 的变化只能来自定义里的 √n 因子。
        let r = 0.001f64;
        let mk = |nt: u32| {
            let n = nt as usize * 3 + 4;
            let mut open = vec![100.0f64; n];
            // delay=1: 3k 处 Buy → 3k+1 开盘成交; 3k+2 处 Close → 3k+3 开盘成交。
            for k in 0..nt as usize {
                let entry = open[3 * k + 1];
                let step = if k % 3 == 2 { -r } else { r };
                open[3 * k + 3] = entry * (1.0 + step);
                if k + 1 < nt as usize {
                    open[3 * (k + 1) + 1] = open[3 * k + 3];
                }
            }
            let mut sig = vec![Signal::Hold; n];
            for k in 0..nt as usize {
                sig[3 * k] = Signal::Buy;
                sig[3 * k + 2] = Signal::Close;
            }
            simulate(
                &open,
                &open,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &ExecConfig::default(),
            )
            .expect("合法回测必须成功")
        };
        let a = mk(30);
        let b = mk(300);
        assert_eq!(a.num_trades, 30);
        assert_eq!(b.num_trades, 300);
        assert!(a.sqn > 0.0 && b.sqn > 0.0, "用例应产生正 SQN");
        let ratio = b.sqn / a.sqn;
        assert!(
            (2.0..5.0).contains(&ratio),
            "SQN 未随 √n 增长(a={}, b={}, ratio={ratio}); 疑似被封顶",
            a.sqn,
            b.sqn
        );
    }

    /// [`log_ret`] 必须在**两侧**都做定义域保护。只查 `prev` 不够: 首根即浮空
    /// 时 `prev` 仍为正而 `curr/prev < 0`, `ln()` 返回 NaN。
    #[test]
    fn test_bar_log_returns_guards_nonpositive_equity() {
        assert_eq!(bar_log_returns(&[100.0, -50.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[100.0, 0.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[-50.0, 100.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[f64::NAN, 100.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[100.0, f64::NAN]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[f64::INFINITY, 100.0]).next(), Some(0.0));
        let r = bar_log_returns(&[100.0, 110.0]).next().unwrap();
        assert!((r - 1.1f64.ln()).abs() < 1e-15);
        for &x in &[100.0, -1.0, 0.0, f64::NAN, 1e-300] {
            assert!(bar_log_returns(&[x, x]).all(|r| r.is_finite()));
        }
    }

    /// L9 / Q2: 非 detailed 路径不物化落盘产物, 但**指标必须逐位一致**。
    ///
    /// 现在这不只是"测出来一致", 而是**类型上不可能不一致**: `compute_metrics`
    /// 的签名收 `&MetricsSource`, 而 `MetricsSource` 里没有 `trades` / 曲线字段 ——
    /// 指标在编译期就看不到 `--detail` 的产物。两者即便未来实现分叉, 也无法让
    /// 指标依赖 detail 侧的数据。
    ///
    /// 本测试锁住"当前实现确实逐位一致", 防止有人绕过 `&MetricsSource` 重接指标源。
    #[test]
    fn non_detailed_metrics_match_detailed_bit_for_bit() {
        let exec = ExecConfig::try_from_bps_fill(1, 2.0, 1.0, false).unwrap();
        for (name, close) in [
            ("单调上涨", make_prices(300)),
            ("锯齿", {
                let mut c = make_prices(300);
                for (i, v) in c.iter_mut().enumerate() {
                    *v *= if i % 2 == 0 { 1.0 } else { 0.99 };
                }
                c
            }),
        ] {
            let mut sig = vec![Signal::Hold; close.len()];
            // 多空交替 -> 同时覆盖 Long / Short / 平仓三种路径
            for i in (0..close.len()).step_by(7) {
                sig[i] = if (i / 7) % 2 == 0 {
                    Signal::Buy
                } else {
                    Signal::Sell
                };
            }
            sig[2] = Signal::Close;
            let a = simulate(
                &close,
                &close,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &exec,
            )
            .expect("合法回测必须成功");
            let b = simulate_detailed(
                &[],
                &close,
                &close,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &exec,
            )
            .expect("合法回测必须成功");
            assert_eq!(a.num_trades, b.summary.num_trades, "{name}: num_trades");
            assert_eq!(b.trades.len(), a.num_trades as usize, "{name}: trades 长度");
            for (k, x) in [
                ("total_return", a.total_return),
                ("sharpe", a.sharpe_ratio),
                ("sortino", a.sortino_ratio),
                ("max_dd", a.max_drawdown_pct),
                ("volatility", a.volatility),
                ("profit_factor", a.profit_factor),
                ("win_rate", a.win_rate),
                ("avg_mae", a.avg_mae),
                ("avg_mfe", a.avg_mfe),
                ("avg_hold_bars", a.avg_hold_bars),
                ("edge_ratio", a.edge_ratio),
                ("sqn", a.sqn),
                ("skewness", a.skewness),
                ("kurtosis", a.kurtosis),
                ("pf_stability", a.pf_stability),
                ("total_fixed_pnl", a.total_fixed_pnl.to_f64()),
                ("fixed_expectancy_bps", a.fixed_expectancy_bps),
                ("fixed_profit_factor", a.fixed_profit_factor),
                ("fixed_sharpe", a.fixed_sharpe_ratio),
                ("fixed_mdd", a.fixed_max_drawdown_pct),
                ("fixed_return", a.fixed_total_return),
            ] {
                let y = match k {
                    "total_return" => b.summary.total_return,
                    "sharpe" => b.summary.sharpe_ratio,
                    "sortino" => b.summary.sortino_ratio,
                    "max_dd" => b.summary.max_drawdown_pct,
                    "volatility" => b.summary.volatility,
                    "profit_factor" => b.summary.profit_factor,
                    "win_rate" => b.summary.win_rate,
                    "avg_mae" => b.summary.avg_mae,
                    "avg_mfe" => b.summary.avg_mfe,
                    "avg_hold_bars" => b.summary.avg_hold_bars,
                    "edge_ratio" => b.summary.edge_ratio,
                    "sqn" => b.summary.sqn,
                    "skewness" => b.summary.skewness,
                    "kurtosis" => b.summary.kurtosis,
                    "pf_stability" => b.summary.pf_stability,
                    "total_fixed_pnl" => b.summary.total_fixed_pnl.to_f64(),
                    "fixed_expectancy_bps" => b.summary.fixed_expectancy_bps,
                    "fixed_profit_factor" => b.summary.fixed_profit_factor,
                    "fixed_sharpe" => b.summary.fixed_sharpe_ratio,
                    "fixed_mdd" => b.summary.fixed_max_drawdown_pct,
                    _ => b.summary.fixed_total_return,
                };
                assert_eq!(
                    x.to_bits(),
                    y.to_bits(),
                    "{name}: {k} 两条路径不一致 —— simulate={x}, simulate_detailed={y}"
                );
            }
        }
    }

    /// **回归 #2**: 现金 <= 0 后必须拒绝再开仓。
    ///
    /// 场景: 100 做空 → 300 平仓, 亏损 2 倍本金, 现金 = -1000; 随后再次做空。
    /// 旧实现直接按 `cash / price` 计算数量, 得到**负数量**(方向反转),
    /// 报告里出现无法解释的仓位与权益。
    #[test]
    fn no_reentry_after_cash_is_wiped_out() {
        let close = vec![100.0, 100.0, 300.0, 300.0, 300.0, 300.0];
        let sig = vec![
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps_fill(1, 0.0, 0.0, true);
        let d = simulate_detailed(
            &[0, 1, 2, 3, 4, 5],
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 1, "爆仓后不得再开仓");
        assert_eq!(d.summary.num_trades, 1);
        assert_eq!(
            d.summary.final_equity,
            crate::money::Amount::from_i64(-1000),
            "现金 = 1000 + 1000 - 3000 = -1000, 且不再产生新仓位"
        );
        for t in &d.trades {
            assert!(
                t.qty > crate::money::Amount::ZERO,
                "持仓数量必须为正(方向由 side 表达): {}",
                t.qty
            );
        }
    }

    /// **回归 #3**: `open` 缺失时成交价退回收盘价, 而不是把整段数据截掉。
    ///
    /// 旧实现 `n = min(close, signals, open)`, `open = []` 时 n = 0,
    /// `fill_at` 里精心写的回退分支**永远不可达**, 报告显示"零交易"。
    #[test]
    fn missing_open_falls_back_to_close_instead_of_truncating_bars() {
        let close = vec![100.0, 110.0, 120.0, 130.0];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let m = simulate(
            &close,
            &[],
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("open 缺失是允许的宽松契约");
        assert_eq!(m.num_trades, 1, "不得因 open 缺失而少回测 bar");
        // bar0 信号 -> bar1 收盘(回退)成交, 末根 130 平仓。
        let expect = (130.0 - 110.0) / 110.0 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
    }

    /// **回归 #7**: `final_equity` 必须来自定点账本, 不得经 f64 曲线回造。
    ///
    /// 旧实现把末点权益先转 `f64` 再 `Amount::from_f64`, 只有约 15 位有效数字;
    /// 与逐笔 `sum(pnl)` 对不上(实测差 1.75e-7)。本用例用 13bp 佣金放大误差。
    #[test]
    fn final_equity_keeps_the_exact_decimal_ledger_value() {
        let price = 100.123_456_789;
        let close = vec![price, price];
        let sig = vec![Signal::Buy, Signal::Hold];
        let initial = crate::money::Amount::from_f64(1_000_000_000.0).expect("有限金额");
        let exec = ExecConfig::from_bps_fill(1, 13.0, 0.0, true);
        let d = simulate_detailed(&[0, 1], &close, &close, &sig, initial, 252.0, &exec)
            .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 1);
        // 开仓与平仓同价 ⇒ 期末权益 = 初始资金 - 两笔佣金, 全部为精确十进制。
        let exact_sum = initial.plus(d.trades[0].pnl).expect("金额累加不得溢出");
        assert_eq!(
            d.summary.final_equity, exact_sum,
            "final_equity 必须与逐笔 pnl 之和精确相等, 不能有 f64 往返误差"
        );
        let qty = initial.div_price(price).expect("正价格");
        let fee = qty
            .mul_price(price)
            .expect("有限价格")
            .mul_ratio(0.0013)
            .expect("有限费率");
        assert_eq!(
            d.summary.final_equity,
            initial.minus(fee).expect("金额").minus(fee).expect("金额")
        );
    }

    /// **回归 #10**: 零盈亏交易是"保本", 不得计入亏损/连亏/胜率分母。
    ///
    /// 序列 `[+1, 0, +2, -1, 0, -2, -3]`: 2 胜 / 3 负 / 2 保本。
    /// 旧实现把两个 0 算作亏损 ⇒ 胜率 2/7、连亏 4、avg_loss 被稀释到 1.2。
    #[test]
    fn breakeven_trades_are_not_losses() {
        let acc = TradeAccum {
            num_trades: 7,
            pnls: vec![1.0, 0.0, 2.0, -1.0, 0.0, -2.0, -3.0],
            total_win: crate::money::Amount::from_i64(3),
            total_loss: crate::money::Amount::from_i64(6),
            ..TradeAccum::default()
        };
        let mut eq = CurveAccum::new(1000.0);
        eq.push(1000.0);
        eq.push(1000.0);
        let mut fx = CurveAccum::new(1000.0);
        fx.push(1000.0);
        fx.push(1000.0);
        let m = compute_metrics(
            &MetricsSource {
                acc,
                eq,
                fx,
                final_equity: crate::money::Amount::from_i64(1000),
            },
            crate::money::Amount::from_i64(1000),
            252.0,
        )
        .expect("指标计算必须成功");
        assert!((m.win_rate - 40.0).abs() < 1e-12, "win_rate={}", m.win_rate);
        assert_eq!(m.consecutive_wins, 2, "保本不得打断连胜");
        assert_eq!(m.consecutive_losses, 3, "保本不得计入/打断连亏");
        assert!(
            (m.avg_loss.to_f64() - 2.0).abs() < 1e-12,
            "avg_loss 只能用 3 笔亏损作分母: {}",
            m.avg_loss
        );
        assert!(
            (m.profit_factor - 0.5).abs() < 1e-12,
            "profit_factor={}",
            m.profit_factor
        );
    }

    /// 构造带止损的详细回测; 默认零费率、零滑点、`delay=1`。
    fn run_stop(
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        signals: &[Signal],
        stop: StopConfig,
        fill_close: bool,
    ) -> SimResult {
        let time: Vec<i64> = (0..close.len() as i64).collect();
        let exec = ExecConfig::try_from_bps_fill(1, 0.0, 0.0, fill_close)
            .expect("零费率/零滑点/延迟 1 必须合法");
        simulate_detailed_ohlc(
            &time,
            high,
            low,
            close,
            open,
            signals,
            crate::money::Amount::from_i64(1_000),
            252.0,
            &exec,
            &stop,
        )
        .expect("合法回测必须成功")
    }

    /// 构造带策略止损线的详细回测; 默认零费率、零滑点、`delay=1`。
    #[allow(clippy::too_many_arguments)]
    fn run_stops(
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        signals: &[Signal],
        stop_lines: Option<&StopLines>,
        stop: StopConfig,
        fill_close: bool,
    ) -> SimResult {
        let time: Vec<i64> = (0..close.len() as i64).collect();
        let exec = ExecConfig::try_from_bps_fill(1, 0.0, 0.0, fill_close)
            .expect("零费率/零滑点/延迟 1 必须合法");
        simulate_detailed_ohlc_with_stops(
            &time,
            high,
            low,
            close,
            open,
            signals,
            stop_lines,
            crate::money::Amount::from_i64(1_000),
            252.0,
            &exec,
            &stop,
        )
        .expect("合法回测必须成功")
    }

    #[test]
    fn exit_long_and_exit_short_only_close_matching_side() {
        assert_eq!(next_target(Side::Long, Signal::ExitLong), Side::Flat);
        assert_eq!(next_target(Side::Short, Signal::ExitLong), Side::Short);
        assert_eq!(next_target(Side::Short, Signal::ExitShort), Side::Flat);
        assert_eq!(next_target(Side::Long, Signal::ExitShort), Side::Long);

        let open = [100.0; 5];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::ExitLong,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &open,
            &open,
            &open,
            &signals,
            StopConfig::default(),
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].entry_time, 1);
        assert_eq!(d.trades[0].exit_time, 3, "ExitLong 必须延迟一根 bar 成交");
        assert_eq!(d.trades[0].exit_reason, "signal");
    }

    #[test]
    fn strategy_stop_line_long_triggers_intrabar() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 101.0];
        let low = [99.0, 99.0, 95.0, 99.0, 99.0];
        let close = [100.0, 100.0, 96.0, 100.0, 100.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let lines = StopLines {
            long: Some(vec![f64::NAN, 96.0, 96.0, 96.0, 96.0]),
            short: None,
        };
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            Some(&lines),
            StopConfig::default(),
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].side, Side::Long);
        assert_eq!(d.trades[0].exit_time, 2);
        assert!((d.trades[0].exit_price - 96.0).abs() < 1e-12);
        assert_eq!(d.trades[0].exit_reason, "strategy_stop");
        assert_eq!(d.summary.strategy_stop_exits, 1);
        assert_eq!(d.summary.stop_loss_exits, 0);
        assert_eq!(d.summary.trailing_stop_exits, 0);
        assert!((d.summary.final_equity.to_f64() - 960.0).abs() < 1e-9);
    }

    #[test]
    fn strategy_stop_line_short_triggers_intrabar() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 105.0, 101.0, 101.0];
        let low = [99.0, 99.0, 99.0, 99.0, 99.0];
        let close = [100.0, 100.0, 104.0, 100.0, 100.0];
        let signals = [
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let lines = StopLines {
            long: None,
            short: Some(vec![f64::NAN, 104.0, 104.0, 104.0, 104.0]),
        };
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            Some(&lines),
            StopConfig::default(),
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].side, Side::Short);
        assert_eq!(d.trades[0].exit_time, 2);
        assert!((d.trades[0].exit_price - 104.0).abs() < 1e-12);
        assert_eq!(d.trades[0].exit_reason, "strategy_stop");
        assert_eq!(d.summary.strategy_stop_exits, 1);
        assert!((d.summary.final_equity.to_f64() - 960.0).abs() < 1e-9);
    }

    #[test]
    fn strategy_stop_line_gap_fills_at_open() {
        let open = [100.0, 100.0, 94.0, 94.0, 94.0];
        let high = [101.0, 101.0, 95.0, 95.0, 95.0];
        let low = [99.0, 99.0, 93.0, 93.0, 93.0];
        let close = [100.0, 100.0, 94.0, 94.0, 94.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let lines = StopLines {
            long: Some(vec![f64::NAN, 96.0, 96.0, 96.0, 96.0]),
            short: None,
        };
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            Some(&lines),
            StopConfig::default(),
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].exit_time, 2);
        assert!(
            (d.trades[0].exit_price - 94.0).abs() < 1e-12,
            "跳空必须按 open 成交, exit={}",
            d.trades[0].exit_price
        );
        assert_eq!(d.trades[0].exit_reason, "strategy_stop");
    }

    #[test]
    fn strategy_stop_line_is_tighter_than_engine_stop() {
        let open = [100.0; 4];
        let high = [101.0, 101.0, 101.0, 101.0];
        let low = [99.0, 98.5, 99.0, 99.0];
        let close = [100.0; 4];
        let signals = [Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let lines = StopLines {
            long: Some(vec![f64::NAN, 99.0, 99.0, 99.0]),
            short: None,
        };
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            Some(&lines),
            StopConfig {
                stop_loss_pct: Some(2.0),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert!((d.trades[0].exit_price - 99.0).abs() < 1e-12);
        assert_eq!(d.trades[0].exit_reason, "strategy_stop");
        assert_eq!(d.summary.strategy_stop_exits, 1);
        assert_eq!(d.summary.stop_loss_exits, 0);
    }

    #[test]
    fn strategy_stop_lines_must_cover_signal_length() {
        let close = [100.0; 3];
        let signals = [Signal::Buy, Signal::Hold, Signal::Hold];
        let lines = StopLines {
            long: Some(vec![99.0]),
            short: None,
        };
        let err = simulate_ohlc_with_stops(
            &close,
            &close,
            &close,
            &close,
            &signals,
            Some(&lines),
            crate::money::Amount::from_i64(1_000),
            252.0,
            &ExecConfig::default(),
            &StopConfig::default(),
        )
        .expect_err("短止损线必须报错");
        assert!(err.to_string().contains("长度不足"), "err={err}");
    }

    #[test]
    fn atr_fixed_stop_uses_prior_completed_bar_atr() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 101.0, 101.0];
        let low = [99.0, 99.0, 99.0, 99.0, 99.0, 95.0];
        let close = [100.0; 6];
        let signals = [
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            None,
            StopConfig {
                atr: Some(AtrStopConfig {
                    period: 2,
                    mult: 2.0,
                    trailing: false,
                }),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].entry_time, 4);
        assert_eq!(d.trades[0].exit_time, 5);
        assert!((d.trades[0].exit_price - 96.0).abs() < 1e-12);
        assert_eq!(d.trades[0].exit_reason, "stop_loss");
        assert_eq!(d.summary.stop_loss_exits, 1);
    }

    #[test]
    fn atr_trailing_stop_ratchets_from_highest_high() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 110.0, 101.0];
        let low = [99.0, 99.0, 99.0, 99.0, 99.0, 90.0];
        let close = [100.0, 100.0, 100.0, 100.0, 105.0, 95.0];
        let signals = [
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            None,
            StopConfig {
                atr: Some(AtrStopConfig {
                    period: 2,
                    mult: 2.0,
                    trailing: true,
                }),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].entry_time, 4);
        // bar4 收盘: extreme=110, ATR=6.5, trail=110-2*6.5=97。
        assert_eq!(d.trades[0].exit_time, 5);
        assert!(
            (d.trades[0].exit_price - 97.0).abs() < 1e-12,
            "exit={}",
            d.trades[0].exit_price
        );
        assert_eq!(d.trades[0].exit_reason, "trailing_stop");
        assert_eq!(d.summary.trailing_stop_exits, 1);
    }

    #[test]
    fn atr_trailing_stop_never_loosens_when_atr_expands() {
        let open = [100.0; 7];
        let high = [101.0, 101.0, 101.0, 101.0, 110.0, 111.0, 105.0];
        let low = [99.0, 99.0, 99.0, 99.0, 99.0, 100.0, 96.0];
        let close = [100.0, 100.0, 100.0, 100.0, 105.0, 105.0, 100.0];
        let signals = [
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            None,
            StopConfig {
                atr: Some(AtrStopConfig {
                    period: 2,
                    mult: 2.0,
                    trailing: true,
                }),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].entry_time, 4);
        // bar4 后 trail=97; bar5 ATR 放大到 8.75, 裸公式会退到 93.5。
        // 追踪止损必须保持 97, 于是 bar6 low=96 仍按 97 触发。
        assert_eq!(d.trades[0].exit_time, 6);
        assert!(
            (d.trades[0].exit_price - 97.0).abs() < 1e-12,
            "exit={}",
            d.trades[0].exit_price
        );
        assert_eq!(d.trades[0].exit_reason, "trailing_stop");
    }

    #[test]
    fn breakeven_stop_moves_to_entry_after_trigger() {
        let open = [100.0, 100.0, 101.0, 101.0];
        let high = [101.0, 104.0, 101.0, 101.0];
        let low = [99.0, 99.0, 99.0, 99.0];
        let close = [100.0, 103.0, 100.0, 100.0];
        let signals = [Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let d = run_stops(
            &open,
            &high,
            &low,
            &close,
            &signals,
            None,
            StopConfig {
                breakeven: Some(BreakevenConfig {
                    trigger_pct: 2.0,
                    offset_pct: 0.0,
                }),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        assert_eq!(d.trades[0].entry_time, 1);
        assert_eq!(d.trades[0].exit_time, 2);
        assert!((d.trades[0].exit_price - 100.0).abs() < 1e-12);
        assert_eq!(d.trades[0].exit_reason, "trailing_stop");
        assert_eq!(d.summary.trailing_stop_exits, 1);
    }

    #[test]
    fn stop_loss_long_triggers_intrabar_and_fills_at_stop() {
        let open = [100.0, 100.0, 100.0, 98.0, 98.0];
        let high = [101.0, 101.0, 100.0, 99.0, 99.0];
        let low = [99.0, 99.0, 97.0, 97.0, 97.0];
        let close = [100.0, 100.0, 98.0, 98.0, 98.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert_eq!(t.side, Side::Long);
        assert!((t.entry_price - 100.0).abs() < 1e-12);
        assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
        assert_eq!(t.exit_reason, "stop_loss");
        assert_eq!(d.summary.stop_loss_exits, 1);
        assert_eq!(d.summary.trailing_stop_exits, 0);
        assert!((d.summary.final_equity.to_f64() - 980.0).abs() < 1e-9);
    }

    #[test]
    fn stop_loss_short_triggers_intrabar_and_fills_at_stop() {
        let open = [100.0, 100.0, 100.0, 102.0, 102.0];
        let high = [101.0, 101.0, 103.0, 103.0, 103.0];
        let low = [99.0, 99.0, 99.0, 101.0, 101.0];
        let close = [100.0, 100.0, 102.0, 102.0, 102.0];
        let signals = [
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert_eq!(t.side, Side::Short);
        assert!((t.entry_price - 100.0).abs() < 1e-12);
        assert!(
            (t.exit_price - 102.0).abs() < 1e-12,
            "exit={}",
            t.exit_price
        );
        assert_eq!(t.exit_reason, "stop_loss");
        assert_eq!(d.summary.stop_loss_exits, 1);
        assert!((d.summary.final_equity.to_f64() - 980.0).abs() < 1e-9);
    }

    #[test]
    fn stop_loss_gap_fills_at_open_not_stop_price() {
        let open = [100.0, 100.0, 95.0, 95.0, 95.0];
        let high = [101.0, 101.0, 96.0, 96.0, 96.0];
        let low = [99.0, 99.0, 94.0, 94.0, 94.0];
        let close = [100.0, 100.0, 95.0, 95.0, 95.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert!(
            (t.exit_price - 95.0).abs() < 1e-12,
            "跳空必须按 open 成交: {}",
            t.exit_price
        );
        assert_eq!(t.exit_reason, "stop_loss");
        assert!((d.summary.final_equity.to_f64() - 950.0).abs() < 1e-9);
    }

    #[test]
    fn stop_exit_still_applies_slippage_and_fees() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 100.0, 100.0, 100.0];
        let low = [99.0, 99.0, 97.0, 97.0, 97.0];
        let close = [100.0, 100.0, 98.0, 98.0, 98.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::try_from_bps_fill(1, 10.0, 5.0, false).expect("费率/滑点合法");
        let d = simulate_detailed_ohlc(
            &[0, 1, 2, 3, 4],
            &high,
            &low,
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(1_000),
            252.0,
            &exec,
            &StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
        )
        .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        // 入场 100×(1+5bp)=100.05, 止损 100.05×0.98=98.049;
        // 平仓再扣 5bp 逆向滑点 ⇒ 97.9999755。
        assert!(
            (t.exit_price - 97.999_975_5).abs() < 1e-9,
            "exit={}",
            t.exit_price
        );
        assert!(t.pnl < crate::money::Amount::ZERO, "止损必须是亏损");
        assert!(
            d.summary.final_equity.to_f64() < 980.0,
            "费用/滑点必须计入止损路径: {}",
            d.summary.final_equity.to_f64()
        );
        assert_eq!(d.summary.stop_loss_exits, 1);
        assert!((d.summary.fee_bps - 10.0).abs() < 1e-12);
        assert!((d.summary.slippage_bps - 5.0).abs() < 1e-12);
    }

    #[test]
    fn trailing_stop_uses_prior_bar_extreme() {
        let open = [100.0, 100.0, 108.0, 108.0, 108.0];
        let high = [101.0, 110.0, 109.0, 109.0, 109.0];
        let low = [99.0, 100.0, 104.0, 104.0, 104.0];
        let close = [100.0, 108.0, 105.0, 105.0, 105.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: None,
                trailing_stop_pct: Some(5.0),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        // bar1 收盘后 extreme=110, 追踪止损=110×0.95=104.5; bar2 盘中触及。
        assert!(
            (t.exit_price - 104.5).abs() < 1e-12,
            "exit={}",
            t.exit_price
        );
        assert_eq!(t.exit_reason, "trailing_stop");
        assert_eq!(d.summary.trailing_stop_exits, 1);
        assert_eq!(d.summary.stop_loss_exits, 0);
        assert!((d.summary.final_equity.to_f64() - 1_045.0).abs() < 1e-9);
    }

    #[test]
    fn trailing_stop_short_ratchets_down_from_lowest_low() {
        let open = [100.0, 100.0, 99.0, 99.0, 99.0];
        let high = [101.0, 101.0, 100.0, 100.0, 100.0];
        let low = [99.0, 95.0, 98.0, 98.0, 98.0];
        let close = [100.0, 99.0, 99.5, 99.5, 99.5];
        let signals = [
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: None,
                trailing_stop_pct: Some(5.0),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert_eq!(t.side, Side::Short);
        // bar1 收盘后 extreme=95, 空头追踪止损=95×1.05=99.75; bar2 盘中触及。
        assert!(
            (t.exit_price - 99.75).abs() < 1e-12,
            "exit={}",
            t.exit_price
        );
        assert_eq!(t.exit_reason, "trailing_stop");
        assert_eq!(d.summary.trailing_stop_exits, 1);
        assert_eq!(d.summary.stop_loss_exits, 0);
        assert!((d.summary.final_equity.to_f64() - 1_002.5).abs() < 1e-9);
    }

    #[test]
    fn fixed_stop_wins_when_it_is_tighter_than_initial_trailing_stop() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 110.0, 100.0, 100.0, 100.0];
        let low = [99.0, 97.5, 99.0, 99.0, 99.0];
        let close = [100.0, 100.0, 100.0, 100.0, 100.0];
        let signals = [
            Signal::Buy,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: Some(5.0),
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        // 入场当根: 固定止损 98 比追踪止损 95 更紧; 即使 high=110, 追踪止损
        // 也只能在**收盘后**更新, 不得回溯到当根。
        assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
        assert_eq!(t.exit_reason, "stop_loss");
        assert_eq!(d.summary.stop_loss_exits, 1);
        assert_eq!(d.summary.trailing_stop_exits, 0);
    }

    #[test]
    fn stop_checks_before_close_fill_signal() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 101.0];
        let low = [99.0, 99.0, 97.0, 99.0, 99.0];
        let close = [100.0, 100.0, 99.0, 99.0, 99.0];
        let signals = [
            Signal::Buy,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            true,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        // bar1 收盘入场; bar2 盘中 stop 先于 bar1 发出的收盘平仓信号。
        assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
        assert_eq!(t.exit_reason, "stop_loss");
    }

    #[test]
    fn entry_at_close_does_not_trigger_stop_on_entry_bar() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 101.0];
        // bar1 的 low=90 发生在收盘入场之前, 不得回溯触发 98 的止损。
        let low = [99.0, 90.0, 99.0, 99.0, 99.0];
        let close = [100.0, 100.0, 100.0, 100.0, 100.0];
        let signals = [
            Signal::Buy,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            true,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert_eq!(t.exit_reason, "signal", "收盘入场当根不得触发止损");
        assert!((t.exit_price - 100.0).abs() < 1e-12);
    }

    #[test]
    fn entry_at_open_can_trigger_stop_on_same_bar() {
        let open = [100.0, 100.0, 100.0, 100.0, 100.0];
        let high = [101.0, 101.0, 101.0, 101.0, 101.0];
        let low = [99.0, 97.0, 99.0, 99.0, 99.0];
        let close = [100.0, 100.0, 100.0, 100.0, 100.0];
        let signals = [
            Signal::Buy,
            Signal::Close,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1);
        let t = &d.trades[0];
        assert_eq!(t.exit_reason, "stop_loss");
        assert!((t.exit_price - 98.0).abs() < 1e-12);
        assert_eq!(t.bars_held, 0, "开盘入场当根止损: 持仓 0 bar");
    }

    #[test]
    fn stop_forces_flat_until_a_new_entry_signal() {
        // 止损后目标被重置为 Flat: 后续 Hold 不得把同一方向“接回来”。
        let open = [100.0; 6];
        let high = [101.0; 6];
        let low = [99.0, 99.0, 97.0, 99.0, 99.0, 99.0];
        let close = [100.0, 100.0, 98.0, 99.0, 99.0, 99.0];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 1, "Hold 不应在止损后自动重入");
        assert_eq!(d.trades[0].exit_reason, "stop_loss");

        // 新 Buy 信号仍可在 delay 后重新入场。
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = run_stop(
            &open,
            &high,
            &low,
            &close,
            &signals,
            StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
            false,
        );
        assert_eq!(d.trades.len(), 2, "新 Buy 必须能重新入场");
        assert_eq!(d.trades[1].entry_time, 4, "新 Buy 在信号后一根 bar 成交");
        assert_eq!(d.trades[1].exit_reason, "end_of_data");
    }

    #[test]
    fn stop_reset_preserves_delay_two_queue_alignment() {
        // delay=2 + fill_close=true: 止损后队列必须用 Flat 占位而不是 clear,
        // 否则下一根新 Buy 会提前一根 bar 成交。
        let open = [100.0; 8];
        let high = [101.0; 8];
        let low = [99.0, 99.0, 99.0, 97.0, 99.0, 99.0, 99.0, 99.0];
        let close = [100.0; 8];
        let signals = [
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec =
            ExecConfig::try_from_bps_fill(2, 0.0, 0.0, true).expect("delay=2 / 零费率必须合法");
        let d = simulate_detailed_ohlc(
            &[0, 1, 2, 3, 4, 5, 6, 7],
            &high,
            &low,
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(1_000),
            252.0,
            &exec,
            &StopConfig {
                stop_loss_pct: Some(2.0),
                trailing_stop_pct: None,
                ..StopConfig::default()
            },
        )
        .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 2);
        assert_eq!(
            d.trades[0].entry_time, 2,
            "信号 bar0 + delay2 在 bar2 收盘成交"
        );
        assert_eq!(d.trades[0].exit_time, 3, "bar3 盘中触发止损");
        assert_eq!(d.trades[0].exit_reason, "stop_loss");
        assert_eq!(
            d.trades[1].entry_time, 6,
            "止损后新 Buy(bar4) 必须等到 bar6 才成交"
        );
        assert_eq!(d.trades[1].exit_reason, "end_of_data");
    }

    #[test]
    fn no_stop_is_bit_identical_to_legacy_simulate() {
        let n = 120;
        let close = make_prices(n);
        let open: Vec<f64> = close.iter().map(|p| p * 0.999).collect();
        let signals: Vec<Signal> = (0..n)
            .map(|i| {
                if i % 17 == 0 {
                    Signal::Buy
                } else if i % 29 == 0 {
                    Signal::Sell
                } else {
                    Signal::Hold
                }
            })
            .collect();
        let exec = ExecConfig::default();
        let a = simulate(
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("legacy simulate");
        let b = simulate_ohlc(
            &close,
            &close,
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
            &StopConfig::default(),
        )
        .expect("no-stop ohlc simulate");
        assert_eq!(a, b, "未配置止损时 high/low 不得改变任何指标");
        assert_eq!(a.stop_loss_exits, 0);
        assert_eq!(a.trailing_stop_exits, 0);
        assert_eq!(a.strategy_stop_exits, 0);

        let da = simulate_detailed(
            &[],
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("legacy detailed");
        let db = simulate_detailed_ohlc(
            &[],
            &close,
            &close,
            &close,
            &open,
            &signals,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
            &StopConfig::default(),
        )
        .expect("no-stop ohlc detailed");
        assert_eq!(da.summary, db.summary);
        assert_eq!(da.trades.len(), db.trades.len());
        assert_eq!(da.equity_curve.len(), db.equity_curve.len());
        assert_eq!(da.fixed_curve, db.fixed_curve);
    }

    /// `exit_reason` 的取值集合必须封闭(落盘层按它做字典序统计)。
    #[test]
    fn exit_reason_values_are_closed_and_wrong() {
        assert_eq!(ExitReason::Signal.as_str(), "signal");
        assert_eq!(ExitReason::StopLoss.as_str(), "stop_loss");
        assert_eq!(ExitReason::TrailingStop.as_str(), "trailing_stop");
        assert_eq!(ExitReason::StrategyStop.as_str(), "strategy_stop");
        assert_eq!(ExitReason::EndOfData.as_str(), "end_of_data");
        let n = 60;
        let close = make_prices(n);
        let mut sig = vec![Signal::Hold; n];
        sig[0] = Signal::Buy;
        // 末根仍持仓 -> 必须标 end_of_data; 中间反手 -> signal
        sig[10] = Signal::Sell;
        // 末根不再给平仓信号 -> 数据末尾强制平仓
        let d = simulate_detailed(
            &[],
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        let reasons: Vec<&str> = d.trades.iter().map(|t| t.exit_reason).collect();
        assert_eq!(reasons, vec!["signal", "end_of_data"]);
        // 非 detailed 路径不得构造 Trade: exit_reason 已是 &'static str, 无堆分配
        for t in &d.trades {
            assert!(
                matches!(t.exit_reason, "signal" | "end_of_data"),
                "未知 exit_reason: {}",
                t.exit_reason
            );
        }
    }
}
