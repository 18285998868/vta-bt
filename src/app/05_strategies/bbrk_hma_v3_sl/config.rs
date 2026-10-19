use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种3止损闩锁版（试点，单参数）：入口同V3，下触逆势多加闩锁+止损，
/// 上触顺势多保持中线离场。网格：σ × bblen × stopk × atrlen 全 Single。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV3SlConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 布林带周期。
    pub bblen: F64Param,
    /// 止损 ATR 倍数。
    pub stopk: F64Param,
    /// ATR 周期。
    pub atrlen: F64Param,
}

impl BbrkHmaV3SlConfig {
    /// 展开后的四元参数组合。
    #[must_use]
    pub fn quads(&self) -> Vec<(f64, usize, f64, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // `saturating_mul`: 笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic。
        let cap = self
            .sigma
            .values()
            .len()
            .saturating_mul(self.bblen.values().len())
            .saturating_mul(self.stopk.values().len())
            .saturating_mul(self.atrlen.values().len());
        let mut out = Vec::with_capacity(cap.min(crate::strategies::MAX_GRID_PREALLOC));
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &k in &self.stopk.values() {
                    for &a in &self.atrlen.values() {
                        out.push((s, b.round() as usize, k, a.round() as usize));
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
        self.bblen
            .validate_range("bbrk_hma_v3_sl.bblen", 2.0, 2000.0)?;
        self.stopk
            .validate_range("bbrk_hma_v3_sl.stopk", 0.5, 10.0)?;
        self.atrlen
            .validate_range("bbrk_hma_v3_sl.atrlen", 2.0, 500.0)?;
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
            bblen: F64Param::Single(20.0),
            stopk: F64Param::Single(2.0),
            atrlen: F64Param::Single(50.0),
        };
        assert_eq!(c.quads(), vec![(2.5, 20, 2.0, 50)]);
    }
}
