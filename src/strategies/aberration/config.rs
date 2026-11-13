use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// Aberration 波动率通道突破(思想级复现，非逐行：上轨=HH(N)+M·ATR(N)，
/// 下轨=LL(N)−M·ATR(N)，穿越翻仓，无日内平仓，长持仓)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AberrationConfig {
    /// 计数/统计窗口(bar 数)。
    pub n: PeriodParam,
    /// 阈值倍数。
    pub mult: F64Param,
}

impl AberrationConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, f64)> {
        let r = self.n.to_range();
        let ns: Vec<usize> = (r.start..=r.end).step_by(r.step).collect();
        let ms = self.mult.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[ns.len(), ms.len()]);
        let mut out = Vec::with_capacity(cap);
        for &a in &ns {
            for &b in &ms {
                out.push((a, b));
            }
        }
        out
    }
}

impl AberrationConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.n.validate()?;
        self.mult.validate_range("aberration.mult", 0.1, 20.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("aberration 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};

    #[test]
    fn pairs_cartesian() {
        let c = AberrationConfig {
            n: PeriodParam::Range(RangeParam {
                start: 20,
                end: 50,
                step: 30,
            }),
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(20, 1.5), (20, 2.5), (50, 1.5), (50, 2.5)]);
    }
}
