use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// ATR 通道突破(中线 SMA20 + ATR14 固定，变 k)。穿越翻仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ATRBandConfig {
    /// 阈值倍数。
    pub mult: F64Param,
}

impl ATRBandConfig {
    /// 展开后的取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.mult.values()
    }
}

impl ATRBandConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.mult.validate_range("atrband.mult", 0.1, 20.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("atrband 参数组合为空".into()));
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
        let c = ATRBandConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
