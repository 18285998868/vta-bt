//! 仓位模式、校验与开仓数量计算。

use serde::Deserialize;

use super::accum::TradeAccum;
use super::stops::atr_at_entry;

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

/// 把名义比例截断到 `[0, max_pct/100]`, 且不超过 1.0(当前引擎无杠杆)。
pub(super) fn clamp_fraction(f: f64, max_pct: f64) -> Option<f64> {
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
pub(super) fn kelly_fraction_from_trades(
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
pub(super) fn entry_fraction(
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
pub(super) fn entry_qty(
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
