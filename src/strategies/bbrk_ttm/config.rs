use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

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
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
    /// Keltner 通道系数。
    pub kc: F64Param,
    /// TTM 挤压回看窗口。
    pub ttmn: PeriodParam,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkTtmConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, f64, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.sigma.values().len(),
            self.bblen.values().len(),
            self.kc.values().len(),
            self.ttmn.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &k in &self.kc.values() {
                    for &n in &self.ttmn.values() {
                        out.push((s, b, k, n));
                    }
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
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_ttm.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_ttm.bblen", 2, 2000)?;
        self.kc.validate_range("bbrk_ttm.kc", 0.5, 5.0)?;
        self.ttmn.validate_range("bbrk_ttm.ttmn", 2, 100)?;
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
            ttmn: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 1.5, 5),
                (2.0, 20, 1.5, 10),
                (2.5, 20, 1.5, 5),
                (2.5, 20, 1.5, 10)
            ]
        );
    }
}
