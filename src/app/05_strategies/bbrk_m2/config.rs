use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// Bollinger Method II 趋势跟随（官方 Method II 手册口径，2026-10-11）：
/// BB(20, sma, σ) + %b>0.8 且 MFI(10)>80 进多头（强价格+强指标确认），
/// 镜像 %b<0.2 且 MFI(10)<20 进空头；MFI 周期取带宽周期之半（官方规则）。
/// 离场中线（原文为修正抛物线/对侧带触碰，为隔离进场差异此处统一中线，
/// 见 strategy.rs）。σ{2.0,2.5,3.0} = 3组+BBRK锚点。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkM2Config {
    pub sigma: F64Param,
}

impl BbrkM2Config {
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

impl BbrkM2Config {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_m2.sigma", 0.1, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("bbrk_m2 参数组合为空".into()));
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
        let c = BbrkM2Config {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
