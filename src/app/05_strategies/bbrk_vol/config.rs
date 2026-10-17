use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+量能门（入场验证#3，用户设计）：
/// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
/// 进场仍 BB突破 + 方向一致（与STR/REX同口径）。
/// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
/// 离场保持中线。σ × len × N × m 四笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkVolConfig {
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    pub voln: F64Param,
    pub volmult: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkVolConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &l in &self.bblen.values() {
                for &n in &self.voln.values() {
                    for &m in &self.volmult.values() {
                        out.push((s, l.round() as usize, n.round() as usize, m));
                    }
                }
            }
        }
        out
    }
}

impl BbrkVolConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_vol.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_vol.bblen", 2.0, 2000.0)?;
        self.voln.validate_range("bbrk_vol.voln", 2.0, 500.0)?;
        self.volmult.validate_range("bbrk_vol.volmult", 0.5, 30.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_vol 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkVolConfig = toml::from_str("sigma = 2.5\nvoln = 10.0\nvolmult = 1.5").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10, 1.5)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkVolConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            voln: F64Param::Range(RangeParam {
                start: 5.0,
                end: 10.0,
                step: 5.0,
            }),
            volmult: F64Param::Single(1.5),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 1.5), (2.5, 20, 10, 1.5)]);
    }
}
