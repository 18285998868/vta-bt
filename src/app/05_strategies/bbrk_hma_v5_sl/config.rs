use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种5止损闩锁版（试点，单参数）：入口同V5，逆势腿加闩锁+止损；
/// 顺势腿保持原规则与中线离场。网格：σ × bblen × hmalen × stopk × atrlen 全 Single。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV5SlConfig {
    pub sigma: F64Param,
    pub bblen: F64Param,
    pub hmalen: F64Param,
    pub stopk: F64Param,
    pub atrlen: F64Param,
}

impl BbrkHmaV5SlConfig {
    pub fn quints(&self) -> Vec<(f64, usize, usize, f64, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &g in &self.hmalen.values() {
                    for &k in &self.stopk.values() {
                        for &a in &self.atrlen.values() {
                            out.push((
                                s,
                                b.round() as usize,
                                g.round() as usize,
                                k,
                                a.round() as usize,
                            ));
                        }
                    }
                }
            }
        }
        out
    }
}

impl BbrkHmaV5SlConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma
            .validate_range("bbrk_hma_v5_sl.sigma", 0.1, 10.0)?;
        self.bblen
            .validate_range("bbrk_hma_v5_sl.bblen", 2.0, 2000.0)?;
        self.hmalen
            .validate_range("bbrk_hma_v5_sl.hmalen", 2.0, 2000.0)?;
        self.stopk
            .validate_range("bbrk_hma_v5_sl.stopk", 0.5, 10.0)?;
        self.atrlen
            .validate_range("bbrk_hma_v5_sl.atrlen", 2.0, 500.0)?;
        if self.quints().is_empty() {
            return Err(AppError::Msg("bbrk_hma_v5_sl 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quints_single() {
        let c = BbrkHmaV5SlConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            hmalen: F64Param::Single(450.0),
            stopk: F64Param::Single(2.0),
            atrlen: F64Param::Single(50.0),
        };
        assert_eq!(c.quints(), vec![(2.5, 20, 450, 2.0, 50)]);
    }
}
