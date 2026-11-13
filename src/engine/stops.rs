//! 止损配置、校验与止损价计算。

use serde::Deserialize;

use super::exec::Side;
use super::scan::valid_fill_price;
use super::sizing::SizingConfig;
use super::types::ExitReason;

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

/// 两个止损价中更紧的一条: 多头取较高价, 空头取较低价。
///
/// 非法价格(非有限/`<= 0`)视为未设置。两边都非法时返回 `None`。
pub(super) fn tighter_stop(side: Side, a: Option<f64>, b: Option<f64>) -> Option<f64> {
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
pub(super) fn wilder_atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
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
pub(super) fn atr_at_entry(atr: &[f64], bar: usize, fill_close: bool) -> Option<f64> {
    let idx = if fill_close { bar } else { bar.checked_sub(1)? };
    atr.get(idx).copied().filter(|v| v.is_finite() && *v > 0.0)
}

/// 当前持仓的有效止损价与原因。
///
/// 同时配置多条规则时取**更紧**的一条: 多头取较高价, 空头取较低价。
/// 价格相同时按固定止损 > 策略止损 > 追踪/保本的顺序归因。
pub(super) fn active_stop(
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
pub(super) fn stop_exit_price(
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
