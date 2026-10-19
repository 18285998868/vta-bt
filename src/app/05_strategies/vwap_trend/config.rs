use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// VWAP顺势（Zarattini & Aziz 2023 SSRN 4631351 口径的加密适配）：
/// 线上持多、线下持空，穿越即反手。`reset` 五种session定义：
/// 0 = 过去24h滚动VWAP（不重置、日内日夜连续），1 = UTC日切重置+午夜平仓，
/// 2/3/4 = 亚(00–08UTC)/欧(08–16)/美(16–24)三分盘：VWAP在盘内累计、
/// 只在盘内持仓（出盘即平+空过盘外bar）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VwapTrendConfig {
    /// 重置模式(0=连续24h, 1=UTC 日切)。
    pub reset: F64Param,
}

impl VwapTrendConfig {
    /// 展开后的模式列表。
    #[must_use]
    pub fn modes(&self) -> Vec<u8> {
        self.reset
            .values()
            .iter()
            .map(|v| v.round() as u8)
            .collect()
    }
}

impl VwapTrendConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.reset.validate_range("vwap_trend.reset", 0.0, 4.0)?;
        if self.modes().is_empty() {
            return Err(AppError::Msg("vwap_trend 参数组合为空".into()));
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
    fn modes_round() {
        let c = VwapTrendConfig {
            reset: F64Param::Range(RangeParam {
                start: 0.0,
                end: 1.0,
                step: 1.0,
            }),
        };
        assert_eq!(c.modes(), vec![0, 1]);
    }
}
