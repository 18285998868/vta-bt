use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// R-Breaker 六价位突破+反转。六价公式固定(枢轴 P=(H+C+L)/3 的 myquant 经典式),
/// 唯一自由参数为止损百分比(原 50 点为 AG 专用, 改百分比通用化)。
/// 日内 UTC 日界强制平仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RBreakerConfig {
    /// 止损百分比。
    pub stop_pct: F64Param,
}

impl RBreakerConfig {
    /// 展开后的取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.stop_pct.values()
    }
}

impl RBreakerConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.stop_pct
            .validate_range("rbreaker.stop_pct", 0.01, 50.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("rbreaker 参数组合为空".into()));
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
    fn values_expand() {
        let c = RBreakerConfig {
            stop_pct: F64Param::Range(RangeParam {
                start: 0.5,
                end: 2.0,
                step: 1.5,
            }),
        };
        assert_eq!(c.values(), vec![0.5, 2.0]);
    }
}
