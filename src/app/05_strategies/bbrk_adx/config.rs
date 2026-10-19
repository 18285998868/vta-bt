use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

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
    pub bblen: F64Param,
    /// ADX周期（缺省14＝旧串语义）。
    #[serde(default = "default_adxlen")]
    /// ADX 周期。
    pub adxlen: F64Param,
    /// ADX 强度阈值。
    pub adx: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

fn default_adxlen() -> F64Param {
    F64Param::Single(14.0)
}

impl BbrkAdxConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let al = self.adxlen.values();
        let ts = self.adx.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                for &a in &al {
                    for &t in &ts {
                        out.push((s, l.round() as usize, a.round() as usize, t));
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
        self.bblen.validate_range("bbrk_adx.bblen", 2.0, 2000.0)?;
        self.adxlen.validate_range("bbrk_adx.adxlen", 2.0, 200.0)?;
        self.adx.validate_range("bbrk_adx.adx", 0.0, 100.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_adx 参数组合为空".into()));
        }
        Ok(())
    }
}

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
            bblen: F64Param::Single(20.0),
            adxlen: F64Param::Single(14.0),
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
