use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+%b离场(@2)：进场与 BBRK 相同（BB20σ突破），
/// 离场改 %b 阈值（多头 %b < exitb 即走，空头 %b > 1−exitb 即走；
/// %b=(close−lower)/(upper−lower)，上轨=1/下轨=0/中线=0.5）。
/// exitb=0.5 恒等于中线离场（内置健全性检查，与 BBRK 逐位一致）。
/// σ × exitb 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkPctbConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 离场缓冲(价格差)。
    pub exitb: F64Param,
}

impl BbrkPctbConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, f64)> {
        let ss = self.sigma.values();
        let bs = self.exitb.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                out.push((s, b));
            }
        }
        out
    }
}

impl BbrkPctbConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_pctb.sigma", 0.1, 10.0)?;
        self.exitb.validate_range("bbrk_pctb.exitb", 0.0, 1.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_pctb 参数组合为空".into()));
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
        let c = BbrkPctbConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            exitb: F64Param::Range(RangeParam {
                start: 0.2,
                end: 0.5,
                step: 0.3,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 0.2), (2.0, 0.5), (2.5, 0.2), (2.5, 0.5)]
        );
    }
}
