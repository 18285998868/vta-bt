use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR追踪离场(@2)：进场与 BBRK 完全相同（BB突破，纯突破无方向门），
/// 离场改用 Chandelier 式追踪（多头 close < 近N根最高high − k·ATR 即走，
/// 空头镜像），不再碰中线就走。追踪窗N=22固定。
/// BB周期bblen（缺省20＝旧串语义）；ATR周期atrlen（缺省14＝旧串语义）。
/// σ × len × atrlen × k 笛卡尔积。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkTrailConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: F64Param,
    /// ATR周期（缺省14＝旧串语义）。
    #[serde(default = "default_atrlen")]
    /// ATR 周期。
    pub atrlen: F64Param,
    /// 跟踪止损 ATR 倍数。
    pub trailk: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

fn default_atrlen() -> F64Param {
    F64Param::Single(14.0)
}

impl BbrkTrailConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let al = self.atrlen.values();
        let ks = self.trailk.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                for &a in &al {
                    for &k in &ks {
                        out.push((s, b.round() as usize, a.round() as usize, k));
                    }
                }
            }
        }
        out
    }
}

impl BbrkTrailConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_trail.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_trail.bblen", 2.0, 2000.0)?;
        self.atrlen
            .validate_range("bbrk_trail.atrlen", 2.0, 500.0)?;
        self.trailk.validate_range("bbrk_trail.trailk", 0.1, 20.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_trail 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_atrlen_default_to_20_14() {
        let c: BbrkTrailConfig = toml::from_str("sigma = 2.5\ntrailk = 3.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 14, 3.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkTrailConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: F64Param::Single(20.0),
            atrlen: F64Param::Single(14.0),
            trailk: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 1.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 14, 2.0),
                (2.0, 20, 14, 3.0),
                (2.5, 20, 14, 2.0),
                (2.5, 20, 14, 3.0)
            ]
        );
    }
}
