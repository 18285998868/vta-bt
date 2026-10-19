use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// RSI(2) Connors 短线反转。buy/sell 笛卡尔积(经典 (10,90)/(5,95) 及交叉对，
/// 预注册在内)。离场：多头 RSI>50 / 空头 RSI<50(均值回归过半，Step2 锁定)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RSI2Config {
    /// 超卖阈值(RSI 量纲)。
    pub buy: F64Param,
    /// 超买阈值(RSI 量纲)。
    pub sell: F64Param,
}

impl RSI2Config {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let bs = self.buy.values();
        let ss = self.sell.values();
        let mut out = Vec::new();
        for &b in &bs {
            for &s in &ss {
                out.push((b, s));
            }
        }
        out
    }
}

impl RSI2Config {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.buy.validate_range("rsi2.buy", 0.0, 100.0)?;
        self.sell.validate_range("rsi2.sell", 0.0, 100.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("rsi2 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = RSI2Config {
            buy: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
            sell: F64Param::Range(RangeParam {
                start: 90.0,
                end: 95.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(5.0, 90.0), (5.0, 95.0), (10.0, 90.0), (10.0, 95.0)]
        );
    }
}
