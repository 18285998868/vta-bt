use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+TTM Squeeze正统版（用户设计2）：布林带上下轨落在
/// Keltner(EMA20 ± k·ATR14) 上下轨内部即 squeeze_on（BB inside KC，与书/ToS一致；
/// 实现见 strategy.rs line144，本注释2026-10-12修正，此前写反但代码一直是对的）；
/// 过去 n 根（含当根）存在 squeeze_on 且当根 BB 突破 → 进；离场保持中线。
/// σ × k × n 三笛卡尔（用户指定 k{1,1.5,2,2.5}；n 取 {5,10} 紧/松两档）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTtmConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// Keltner 通道系数。
    pub kc: F64Param,
    /// TTM 挤压回看窗口。
    pub ttmn: F64Param,
}

impl BbrkTtmConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, f64, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // `saturating_mul`: 笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic。
        let cap = self
            .sigma
            .values()
            .len()
            .saturating_mul(self.kc.values().len())
            .saturating_mul(self.ttmn.values().len());
        let mut out = Vec::with_capacity(cap.min(crate::strategies::MAX_GRID_PREALLOC));
        for &s in &self.sigma.values() {
            for &k in &self.kc.values() {
                for &n in &self.ttmn.values() {
                    out.push((s, k, n.round() as usize));
                }
            }
        }
        out
    }
}

impl BbrkTtmConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_ttm.sigma", 0.1, 10.0)?;
        self.kc.validate_range("bbrk_ttm.kc", 0.5, 5.0)?;
        self.ttmn.validate_range("bbrk_ttm.ttmn", 2.0, 100.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_ttm 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkTtmConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            kc: F64Param::Single(1.5),
            ttmn: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 1.5, 5), (2.0, 1.5, 10), (2.5, 1.5, 5), (2.5, 1.5, 10)]
        );
    }
}
