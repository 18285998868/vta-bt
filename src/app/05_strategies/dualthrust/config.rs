use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// Dual Thrust 开盘区间突破。N=回看天数, k=k1=k2 对称系数(经典默认 0.2)。
/// Range=max(HH−LC, HC−LL), 上轨=当日开+k·R, 下轨=当日开−k·R。
/// 持仓过夜(与 myquant 示例一致, Step2 已锁定)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualThrustConfig {
    /// 计数/统计窗口(bar 数)。
    pub n: PeriodParam,
    /// 阈值倍数(常为 ATR / 通道倍数)。
    pub k: F64Param,
}

impl DualThrustConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, f64)> {
        let r = self.n.to_range();
        let ns: Vec<usize> = (r.start..=r.end).step_by(r.step).collect();
        let ks = self.k.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // `saturating_mul`: 笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic。
        let cap = ns.len().saturating_mul(ks.len());
        let mut out = Vec::with_capacity(cap.min(crate::strategies::MAX_GRID_PREALLOC));
        for &a in &ns {
            for &b in &ks {
                out.push((a, b));
            }
        }
        out
    }
}

impl DualThrustConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.n.validate()?;
        self.k.validate_range("dualthrust.k", 0.01, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("dualthrust 参数组合为空".into()));
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
        let c = DualThrustConfig {
            n: PeriodParam::Range(RangeParam {
                start: 3,
                end: 5,
                step: 2,
            }),
            k: F64Param::Range(RangeParam {
                start: 0.1,
                end: 0.2,
                step: 0.1,
            }),
        };
        assert_eq!(c.pairs(), vec![(3, 0.1), (3, 0.2), (5, 0.1), (5, 0.2)]);
    }
}
