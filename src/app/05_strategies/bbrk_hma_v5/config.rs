use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种5（试点，单参数）：HMA之上碰上轨**或**碰下轨都做多，
/// HMA之下碰下轨**或**碰上轨都做空（V1+V2并集）；离场统一中线。
/// 网格：σ × bblen × hmalen 全 Single（试点单组）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV5Config {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 布林带周期。
    pub bblen: F64Param,
    /// HMA regime 周期。
    pub hmalen: F64Param,
}

impl BbrkHmaV5Config {
    /// 展开后的三元参数组合。
    pub fn triples(&self) -> Vec<(f64, usize, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &g in &self.hmalen.values() {
                    out.push((s, b.round() as usize, g.round() as usize));
                }
            }
        }
        out
    }
}

impl BbrkHmaV5Config {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_hma_v5.sigma", 0.1, 10.0)?;
        self.bblen
            .validate_range("bbrk_hma_v5.bblen", 2.0, 2000.0)?;
        self.hmalen
            .validate_range("bbrk_hma_v5.hmalen", 2.0, 2000.0)?;
        if self.triples().is_empty() {
            return Err(AppError::Msg("bbrk_hma_v5 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV5Config {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            hmalen: F64Param::Single(450.0),
        };
        assert_eq!(c.triples(), vec![(2.5, 20, 450)]);
    }
}
