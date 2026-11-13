use crate::error::AppError;
use serde::Deserialize;

use crate::config::PeriodParam;

/// 交易方向模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendMode {
    /// 只做多: 空头突破信号被忽略。
    LongOnly,
    /// 多空双向: 下跌趋势中跌破入场下轨做空。
    LongShort,
}

impl TrendMode {
    /// 报告/参数串用标签。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::LongOnly => "long_only",
            Self::LongShort => "long_short",
        }
    }
}

/// 单值或列表: `mode = "long_only"` 与 `mode = ["long_only", "long_short"]` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ModeList {
    /// 单值形态。
    One(TrendMode),
    /// 列表形态。
    Many(Vec<TrendMode>),
}

impl ModeList {
    /// 归一化为列表。
    #[must_use]
    pub fn as_vec(&self) -> Vec<TrendMode> {
        match self {
            Self::One(m) => vec![*m],
            Self::Many(v) => v.clone(),
        }
    }
}

impl Default for ModeList {
    fn default() -> Self {
        Self::One(TrendMode::LongShort)
    }
}

/// Clenow 混合趋势: 双均线 regime 过滤 + 唐奇安通道突破。
///
/// 来自《Following the Trend》的核心组合: 50/100 均线判方向, 20 日通道
/// 突破入场, 10 日反向通道离场; ATR 风险仓位与追踪止损由引擎的
/// `[[strategy]].sizing` / `[[strategy]].stop` 配置, 策略只负责信号。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClenowConfig {
    /// regime 快线周期。
    pub fast: PeriodParam,
    /// regime 慢线周期。
    pub slow: PeriodParam,
    /// 入场通道周期(收盘突破前 N 根最高/最低价)。
    pub entry: PeriodParam,
    /// 离场通道周期(收盘反向突破前 N 根极值)。
    pub exit: PeriodParam,
    /// 方向模式; 单值或列表。缺省只跑双向。
    #[serde(default)]
    pub mode: ModeList,
}

/// 一组展开后的参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClenowParams {
    /// regime 快线周期。
    pub fast: usize,
    /// regime 慢线周期。
    pub slow: usize,
    /// 入场通道周期。
    pub entry: usize,
    /// 离场通道周期。
    pub exit: usize,
    /// 方向模式。
    pub mode: TrendMode,
}

impl ClenowConfig {
    /// 展开后的参数组合(只保留 `fast < slow` 的有效 regime)。
    #[must_use]
    pub fn combos(&self) -> Vec<ClenowParams> {
        let expand = |p: &PeriodParam| -> Vec<usize> {
            let r = p.to_range();
            (r.start..=r.end).step_by(r.step).collect()
        };
        let fasts = expand(&self.fast);
        let slows = expand(&self.slow);
        let entries = expand(&self.entry);
        let exits = expand(&self.exit);
        let modes = self.mode.as_vec();
        let cap = crate::strategies::grid_prealloc(&[
            fasts.len(),
            slows.len(),
            entries.len(),
            exits.len(),
            modes.len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &f in &fasts {
            for &s in &slows {
                if f >= s {
                    continue;
                }
                for &e in &entries {
                    for &x in &exits {
                        for &mode in &modes {
                            out.push(ClenowParams {
                                fast: f,
                                slow: s,
                                entry: e,
                                exit: x,
                                mode,
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// 语义校验: 各周期范围合法、方向模式非空、展开后至少一组参数。
    ///
    /// # Errors
    /// 任一周期越界、`mode` 为空或没有 `fast < slow` 组合时返回 `AppError`。
    pub fn validate(&self) -> crate::error::Result<()> {
        for p in [&self.fast, &self.slow, &self.entry, &self.exit] {
            p.validate()?;
        }
        if self.mode.as_vec().is_empty() {
            return Err(AppError::Msg("clenow.mode 不能为空".into()));
        }
        if self.combos().is_empty() {
            return Err(AppError::Msg("clenow 无 fast<slow 有效组合".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    fn cfg() -> ClenowConfig {
        ClenowConfig {
            fast: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
            slow: PeriodParam::Range(RangeParam {
                start: 100,
                end: 200,
                step: 100,
            }),
            entry: PeriodParam::Range(RangeParam {
                start: 20,
                end: 55,
                step: 35,
            }),
            exit: PeriodParam::Range(RangeParam {
                start: 10,
                end: 20,
                step: 10,
            }),
            mode: ModeList::Many(vec![TrendMode::LongShort, TrendMode::LongOnly]),
        }
    }

    #[test]
    fn combos_cartesian_with_regime_and_modes() {
        let c = cfg();
        let combos = c.combos();
        // fast<slow: (50,100),(50,200),(100,200) = 3; entry×exit×mode = 2×2×2 = 8。
        assert_eq!(combos.len(), 24);
        assert!(combos.iter().all(|p| p.fast < p.slow));
        assert_eq!(combos[0].mode, TrendMode::LongShort);
        assert_eq!(combos[1].mode, TrendMode::LongOnly);
    }

    #[test]
    fn default_mode_is_long_short() {
        let c: ClenowConfig =
            toml::from_str("fast = 50\nslow = 100\nentry = 20\nexit = 10\n").expect("配置解析");
        assert_eq!(c.mode.as_vec(), vec![TrendMode::LongShort]);
    }

    #[test]
    fn validate_rejects_empty_mode() {
        let mut c = cfg();
        c.mode = ModeList::Many(vec![]);
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_no_regime_combo() {
        let c = ClenowConfig {
            fast: PeriodParam::Single(100),
            slow: PeriodParam::Single(100),
            entry: PeriodParam::Single(20),
            exit: PeriodParam::Single(10),
            mode: ModeList::One(TrendMode::LongShort),
        };
        assert!(c.validate().is_err());
    }
}
