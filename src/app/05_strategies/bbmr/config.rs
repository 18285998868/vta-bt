use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BB 均值回归(官方 `BBMeanReversion` 口径): BB(20,σ)+RSI(14)，
/// 碰下沿且 RSI<0.30 做多，碰上沿且 RSI>0.70 做空，回到中线平仓。
/// σ 变；RSI 阈值固定(官方值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBMRConfig {
    pub sigma: F64Param,
}

impl BBMRConfig {
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

impl BBMRConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbmr.sigma", 0.1, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("bbmr 参数组合为空".into()));
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
        let c = BBMRConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
