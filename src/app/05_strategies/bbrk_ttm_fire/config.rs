use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+TTM fire+动量版（书中口径改编）：挤压检测与 v1 逐位一致
/// （BB inside KC，EMA20+ATR14 KC，离场中线），只改入口——
/// fire首根（当根squeeze_off且前根squeeze_on）＋ 动量直方图同向
/// （Donchian20中轴+SMA20→delta→linreg20平滑，多头hist>0且rising）
/// 才进。动量周期20书中默认，预注册固定不调参。
/// σ × k 笛卡尔积（k{1,1.5,2,2.5}与v1同口径可比）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTtmFireConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
    /// Keltner 通道系数。
    pub kc: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkTtmFireConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, f64)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.sigma.values().len(),
            self.bblen.values().len(),
            self.kc.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &k in &self.kc.values() {
                    out.push((s, b, k));
                }
            }
        }
        out
    }
}

impl BbrkTtmFireConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma
            .validate_range("bbrk_ttm_fire.sigma", 0.1, 10.0)?;
        self.kc.validate_range("bbrk_ttm_fire.kc", 0.5, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_ttm_fire 参数组合为空".into()));
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
        let c = BbrkTtmFireConfig {
            sigma: F64Param::Single(2.5),
            kc: F64Param::Range(RangeParam {
                start: 1.0,
                end: 1.5,
                step: 0.5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 1.0), (2.5, 20, 1.5)]);
    }
}
