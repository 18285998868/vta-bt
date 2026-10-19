use crate::config::F64Param;
use crate::error::AppError;
use crate::strategies::mabb_short::MABB_MA_NAMES;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// 均线 + ATR 通道策略配置。
pub struct MaAtrConfig {
    /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
    pub ma: Vec<String>,
    /// 均线周期。
    pub len: F64Param,
    /// 阈值倍数。
    pub mult: F64Param,
    /// ATR 周期。
    pub atrlen: F64Param,
}

impl MaAtrConfig {
    /// 展开后的五元参数组合。
    pub fn quints(&self) -> Vec<(String, usize, f64, usize)> {
        let mut out = Vec::new();
        for name in &self.ma {
            for &l in &self.len.values() {
                for &k in &self.mult.values() {
                    for &a in &self.atrlen.values() {
                        out.push((name.clone(), l.round() as usize, k, a.round() as usize));
                    }
                }
            }
        }
        out
    }

    /// 语义校验: 参数范围与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.len.validate_range("ma_atr.len", 1.0, 2000.0)?;
        self.mult.validate_range("ma_atr.mult", 0.1, 20.0)?;
        self.atrlen.validate_range("ma_atr.atrlen", 2.0, 500.0)?;
        for name in &self.ma {
            if !MABB_MA_NAMES.contains(&name.as_str()) {
                return Err(AppError::Msg(format!(
                    "ma_atr 未知均线 {name:?}, 可选 {MABB_MA_NAMES:?}"
                )));
            }
        }
        if self.quints().is_empty() {
            return Err(AppError::Msg("ma_atr 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quints_cartesian() {
        let c = MaAtrConfig {
            ma: vec!["sma".into()],
            len: F64Param::Single(20.0),
            mult: F64Param::Single(2.0),
            atrlen: F64Param::Single(14.0),
        };
        assert_eq!(c.quints(), vec![("sma".to_string(), 20, 2.0, 14)]);
    }
}
