use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK 布林趋势突破(WikiFX-EA口径): BB(20,σ)收盘突破上轨做多、
/// 跌破下轨做空，回到中线平仓。σ 变；周期固定20(布林经典值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBRKConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
}

impl BBRKConfig {
    /// 展开后的取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

impl BBRKConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk.sigma", 0.1, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("bbrk 参数组合为空".into()));
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
        let c = BBRKConfig {
            sigma: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![1.5, 2.0, 2.5]);
    }
}
