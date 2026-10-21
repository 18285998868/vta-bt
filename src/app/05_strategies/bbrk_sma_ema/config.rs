use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+SMA regime门（EMA基线版：BB中轨ema，门线仍SMA(L)，其余与 bbrk_sma 逐位一致）：
/// close > SMA(L) 只允许多头（空头突破拦掉），close < SMA(L) 只允许空头；
/// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
/// 离场保持中线。SMA(L) 手算（与 BB 中线 SMA20 同口径），NaN/预热不足 → 拦。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkSmaEmaConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// SMA regime 周期。
    pub smareg: PeriodParam,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkSmaEmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ls = self.smareg.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[ss.len(), bs.len(), ls.len()]);
        let mut out = Vec::with_capacity(cap);
        for &s in &ss {
            for &b in &bs {
                for &l in &ls {
                    out.push((s, b, l));
                }
            }
        }
        out
    }
}

impl BbrkSmaEmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_sma_ema.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_sma_ema.bblen", 2, 2000)?;
        self.smareg.validate_range("bbrk_sma_ema.smareg", 2, 2000)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_sma_ema 参数组合为空".into()));
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
        let c = BbrkSmaEmaConfig {
            sigma: F64Param::Single(2.5),
            smareg: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
