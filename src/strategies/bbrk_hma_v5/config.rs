use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// HMA方向变种5（试点，单参数）：HMA之上碰上轨**或**碰下轨都做多，
/// HMA之下碰下轨**或**碰上轨都做空（V1+V2并集）；离场统一中线。
/// 网格：σ × bblen × hmalen 全 Single（试点单组）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV5Config {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// HMA regime 周期。
    pub hmalen: PeriodParam,
}

impl BbrkHmaV5Config {
    /// 展开后的三元参数组合。
    #[must_use]
    pub fn triples(&self) -> Vec<(f64, usize, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.sigma.values().len(),
            self.bblen.values().len(),
            self.hmalen.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &g in &self.hmalen.values() {
                    out.push((s, b, g));
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
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_hma_v5.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_hma_v5.bblen", 2, 2000)?;
        self.hmalen.validate_range("bbrk_hma_v5.hmalen", 2, 2000)?;
        if self.triples().is_empty() {
            return Err(AppError::Msg("bbrk_hma_v5 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV5Config {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            hmalen: PeriodParam::Single(450),
        };
        assert_eq!(c.triples(), vec![(2.5, 20, 450)]);
    }
}
