use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+ADX 震荡过滤(BBRK 突破 + ADX 门限): 只在 `adx >= thr` 时允许开仓，
/// 回到中线无条件平仓。σ × len × adxlen × thr 笛卡尔积。
/// BB周期bblen（缺省20＝旧串语义）；ADX周期adxlen（缺省14＝旧串语义，经典值）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAdxConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// ADX周期（缺省14＝旧串语义）。
    #[serde(default = "default_adxlen")]
    /// ADX 周期。
    pub adxlen: PeriodParam,
    /// ADX 强度阈值。
    pub adx: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

fn default_adxlen() -> PeriodParam {
    PeriodParam::Single(14)
}

impl BbrkAdxConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let al = self.adxlen.values();
        let ts = self.adx.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[ss.len(), ls.len(), al.len(), ts.len()]);
        let mut out = Vec::with_capacity(cap);
        for &s in &ss {
            for &l in &ls {
                for &a in &al {
                    for &t in &ts {
                        out.push((s, l, a, t));
                    }
                }
            }
        }
        out
    }
}

impl BbrkAdxConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_adx.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_adx.bblen", 2, 2000)?;
        self.adxlen.validate_range("bbrk_adx.adxlen", 2, 200)?;
        self.adx.validate_range("bbrk_adx.adx", 0.0, 100.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_adx 参数组合为空".into()));
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
    fn missing_bblen_adxlen_default_to_20_14() {
        let c: BbrkAdxConfig = toml::from_str("sigma = 2.5\nadx = 20.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 14, 20.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAdxConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            adxlen: PeriodParam::Single(14),
            adx: F64Param::Range(RangeParam {
                start: 20.0,
                end: 25.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 14, 20.0),
                (2.0, 20, 14, 25.0),
                (2.5, 20, 14, 20.0),
                (2.5, 20, 14, 25.0)
            ]
        );
    }
}
