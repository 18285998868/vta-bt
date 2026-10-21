use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// HMA方向变种3止损闩锁版（试点，单参数）：入口同V3，下触逆势多加闩锁+止损，
/// 上触顺势多保持中线离场。网格：σ × bblen × stopk × atrlen 全 Single。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV3SlConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// 止损 ATR 倍数。
    pub stopk: F64Param,
    /// ATR 周期。
    pub atrlen: PeriodParam,
}

impl BbrkHmaV3SlConfig {
    /// 展开后的四元参数组合。
    #[must_use]
    pub fn quads(&self) -> Vec<(f64, usize, f64, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.sigma.values().len(),
            self.bblen.values().len(),
            self.stopk.values().len(),
            self.atrlen.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &k in &self.stopk.values() {
                    for &a in &self.atrlen.values() {
                        out.push((s, b, k, a));
                    }
                }
            }
        }
        out
    }
}

impl BbrkHmaV3SlConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma
            .validate_range("bbrk_hma_v3_sl.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_hma_v3_sl.bblen", 2, 2000)?;
        self.stopk
            .validate_range("bbrk_hma_v3_sl.stopk", 0.5, 10.0)?;
        self.atrlen
            .validate_range("bbrk_hma_v3_sl.atrlen", 2, 500)?;
        if self.quads().is_empty() {
            return Err(AppError::Msg("bbrk_hma_v3_sl 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_single() {
        let c = BbrkHmaV3SlConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            stopk: F64Param::Single(2.0),
            atrlen: PeriodParam::Single(50),
        };
        assert_eq!(c.quads(), vec![(2.5, 20, 2.0, 50)]);
    }
}
