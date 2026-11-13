use crate::error::AppError;
use serde::Deserialize;

use crate::config::PeriodParam;

/// 海龟 Donchian 突破。entry/exit 独立笛卡尔(经典 20/10 与 55/20 皆在其中)。
/// 策略本身只产出信号; ATR 风险仓位由 `[[strategy]].sizing` 的 `type = "atr"`
/// 配置(见 [`crate::engine::SizingConfig`]), 不再需要固定名义近似。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurtleConfig {
    /// 入场通道周期。
    pub entry: PeriodParam,
    /// 离场通道周期。
    pub exit: PeriodParam,
}

impl TurtleConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let e = self.entry.to_range();
        let x = self.exit.to_range();
        let es: Vec<usize> = (e.start..=e.end).step_by(e.step).collect();
        let xs: Vec<usize> = (x.start..=x.end).step_by(x.step).collect();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[es.len(), xs.len()]);
        let mut out = Vec::with_capacity(cap);
        for &a in &es {
            for &b in &xs {
                out.push((a, b));
            }
        }
        out
    }
}

impl TurtleConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        for pp in [&self.entry, &self.exit] {
            pp.validate()?;
        }
        if self.pairs().is_empty() {
            return Err(AppError::Msg("turtle 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian_entry_exit() {
        let c = TurtleConfig {
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
        };
        assert_eq!(c.pairs(), vec![(20, 10), (20, 20), (55, 10), (55, 20)]);
    }
}
