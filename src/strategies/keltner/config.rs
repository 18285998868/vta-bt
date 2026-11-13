use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// Keltner(EMA20 ± k·ATR14)穿越，k 变。与 ATRBand 唯一变量是中线，逐格对照。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeltnerConfig {
    /// 阈值倍数。
    pub mult: F64Param,
}

impl KeltnerConfig {
    /// 展开后的取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.mult.values()
    }
}

impl KeltnerConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.mult.validate_range("keltner.mult", 0.1, 20.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("keltner 参数组合为空".into()));
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
        let c = KeltnerConfig {
            mult: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
