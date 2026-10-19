use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// VWAP拉伸回归：session VWAP(日界重置, 典型价加权)偏离超 th·ATR后首次收回。
/// th 变；多空对称同一阈值。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VwapConfig {
    /// 方向判定阈值。
    pub thresh: F64Param,
}

impl VwapConfig {
    /// 展开后的取值列表。
    pub fn values(&self) -> Vec<f64> {
        self.thresh.values()
    }
}

impl VwapConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.thresh.validate_range("vwap.thresh", 0.5, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("vwap 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = VwapConfig {
            thresh: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 1.0,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.5]);
    }
}
