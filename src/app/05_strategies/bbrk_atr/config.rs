use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+ATR扩张门（入场验证#4）：进场仍 BB突破 + 方向一致，
/// 叠加 `ATR(atr1)/ATR(atr2) > thr`（Wilder口径）。
/// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × len × atr1 × atr2 × thr 笛卡尔积，
/// 只取 atr1<atr2（短长语义，ema_cross同口径）。
/// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
/// ATR短长窗（缺省5/20＝旧串语义，经典短长配）。
/// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAtrConfig {
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    /// ATR短窗（缺省5＝旧串语义）。
    #[serde(default = "default_atr1")]
    pub atr1: F64Param,
    /// ATR长窗（缺省20＝旧串语义）。
    #[serde(default = "default_atr2")]
    pub atr2: F64Param,
    pub atrthr: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

fn default_atr1() -> F64Param {
    F64Param::Single(5.0)
}

fn default_atr2() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkAtrConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let a1 = self.atr1.values();
        let a2 = self.atr2.values();
        let ts = self.atrthr.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                for &x in &a1 {
                    for &y in &a2 {
                        let (xi, yi) = (x.round() as usize, y.round() as usize);
                        if xi >= yi {
                            continue;
                        }
                        for &t in &ts {
                            out.push((s, l.round() as usize, xi, yi, t));
                        }
                    }
                }
            }
        }
        out
    }
}

impl BbrkAtrConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_atr.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_atr.bblen", 2.0, 2000.0)?;
        self.atr1.validate_range("bbrk_atr.atr1", 2.0, 500.0)?;
        self.atr2.validate_range("bbrk_atr.atr2", 2.0, 500.0)?;
        self.atrthr.validate_range("bbrk_atr.atrthr", 0.1, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_atr 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_atr_defaults_to_20_5_20() {
        let c: BbrkAtrConfig = toml::from_str("sigma = 2.5\natrthr = 1.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 20, 1.0)]);
    }

    #[test]
    fn short_not_less_than_long_filtered() {
        let c: BbrkAtrConfig =
            toml::from_str("sigma = 2.5\nbblen = 20.0\natr1 = 10.0\natr2 = 10.0\natrthr = 1.0")
                .unwrap();
        assert!(c.pairs().is_empty());
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAtrConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            atr1: F64Param::Single(5.0),
            atr2: F64Param::Single(20.0),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.5, 20, 5, 20, 0.8), (2.5, 20, 5, 20, 1.0)]
        );
    }
}
