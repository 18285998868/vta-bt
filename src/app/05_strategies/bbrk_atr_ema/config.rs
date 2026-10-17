use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR扩张门（EMA基线版：BB中轨ema，其余与 bbrk_atr 逐位一致）：进场仍 BB20σ突破 + 方向一致，
/// 叠加 `ATR5/ATR20 > thr`（Wilder口径，长度5/20固定）。
/// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × thr 笛卡尔积。
/// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAtrEmaConfig {
    pub sigma: F64Param,
    pub atrthr: F64Param,
}

impl BbrkAtrEmaConfig {
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let ts = self.atrthr.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &t in &ts {
                out.push((s, t));
            }
        }
        out
    }
}

impl BbrkAtrEmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_atr_ema.sigma", 0.1, 10.0)?;
        self.atrthr
            .validate_range("bbrk_atr_ema.atrthr", 0.1, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_atr_ema 参数组合为空".into()));
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
        let c = BbrkAtrEmaConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 0.8), (2.0, 1.0), (2.5, 0.8), (2.5, 1.0)]
        );
    }
}
