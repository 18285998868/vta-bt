use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+量能门（EMA基线版：BB中轨ema，其余与 bbrk_vol 逐位一致）：
/// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
/// 进场仍 BB20σ突破 + 方向一致（与STR/REX同口径）。
/// 离场保持中线。σ × N × m 三笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkVolEmaConfig {
    pub sigma: F64Param,
    pub voln: F64Param,
    pub volmult: F64Param,
}

impl BbrkVolEmaConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, f64)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &n in &self.voln.values() {
                for &m in &self.volmult.values() {
                    out.push((s, n.round() as usize, m));
                }
            }
        }
        out
    }
}

impl BbrkVolEmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_vol_ema.sigma", 0.1, 10.0)?;
        self.voln.validate_range("bbrk_vol_ema.voln", 2.0, 500.0)?;
        self.volmult
            .validate_range("bbrk_vol_ema.volmult", 0.5, 30.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_vol_ema 参数组合为空".into()));
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
        let c = BbrkVolEmaConfig {
            sigma: F64Param::Single(2.5),
            voln: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
            volmult: F64Param::Single(1.5),
        };
        assert_eq!(c.pairs(), vec![(2.5, 5, 1.5), (2.5, 10, 1.5)]);
    }
}
