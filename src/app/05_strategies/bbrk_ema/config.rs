use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+EMA regime门：SMA门同形，regime线换 EMA（vector-ta 现成指标）。
/// close > EMA(L) 只允许多头，close < EMA(L) 只允许空头；
/// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
/// BB周期bblen（缺省20＝旧串语义）。
/// 离场保持中线。EMA(L) NaN/预热不足 → 拦。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkEmaConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// EMA regime 周期。
    pub emareg: PeriodParam,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkEmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ls = self.emareg.values();
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

impl BbrkEmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_ema.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_ema.bblen", 2, 2000)?;
        self.emareg.validate_range("bbrk_ema.emareg", 2, 2000)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_ema 参数组合为空".into()));
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
    fn missing_bblen_defaults_to_20() {
        let c: BbrkEmaConfig = toml::from_str("sigma = 2.5\nemareg = 100").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 100)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkEmaConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            emareg: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
