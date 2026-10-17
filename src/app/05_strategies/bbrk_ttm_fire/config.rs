use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+TTM fire+动量版（书中口径改编）：挤压检测与 v1 逐位一致
/// （BB inside KC，EMA20+ATR14 KC，离场中线），只改入口——
/// fire首根（当根squeeze_off且前根squeeze_on）＋ 动量直方图同向
/// （Donchian20中轴+SMA20→delta→linreg20平滑，多头hist>0且rising）
/// 才进。动量周期20书中默认，预注册固定不调参。
/// σ × k 笛卡尔积（k{1,1.5,2,2.5}与v1同口径可比）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTtmFireConfig {
    pub sigma: F64Param,
    pub kc: F64Param,
}

impl BbrkTtmFireConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &k in &self.kc.values() {
                out.push((s, k));
            }
        }
        out
    }
}

impl BbrkTtmFireConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma
            .validate_range("bbrk_ttm_fire.sigma", 0.1, 10.0)?;
        self.kc.validate_range("bbrk_ttm_fire.kc", 0.5, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_ttm_fire 参数组合为空".into()));
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
        let c = BbrkTtmFireConfig {
            sigma: F64Param::Single(2.5),
            kc: F64Param::Range(RangeParam {
                start: 1.0,
                end: 1.5,
                step: 0.5,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 1.0), (2.5, 1.5)]);
    }
}
