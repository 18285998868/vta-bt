use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种1（试点，单参数）：HMA之上，碰上轨**或**碰下轨都做多；
/// HMA之下保持原规则（下破做空）。离场统一中线（顺势进场的中线离场
/// 恰是逆势进场的均值回归目标，故不另设离场以隔离入口变量）。
/// 网格：σ × bblen × hmalen 全 Single（试点单组）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV1Config {
    pub sigma: F64Param,
    pub bblen: F64Param,
    pub hmalen: F64Param,
}

impl BbrkHmaV1Config {
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

impl BbrkHmaV1Config {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_hma_v1.sigma", 0.1, 10.0)?;
        self.bblen
            .validate_range("bbrk_hma_v1.bblen", 2.0, 2000.0)?;
        self.hmalen
            .validate_range("bbrk_hma_v1.hmalen", 2.0, 2000.0)?;
        if self.triples().is_empty() {
            return Err(AppError::Msg("bbrk_hma_v1 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV1Config {
            sigma: F64Param::Single(3.5),
            bblen: F64Param::Single(25.0),
            hmalen: F64Param::Single(450.0),
        };
        assert_eq!(c.triples(), vec![(3.5, 25, 450)]);
    }
}
