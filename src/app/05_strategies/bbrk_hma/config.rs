use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+HMA regime门（入场验证#6）：SMA门嫌滞后，换 Hull 均线
/// （平滑+低滞后，vector-ta 现成指标直接用）。
/// close > HMA(L) 只允许多头，close < HMA(L) 只允许空头，相等不开仓。
/// L{50,100,200,400} × σ3档 = 12组+BBRK对照，与 SMA 版逐位可比。
/// BB周期bblen（缺省20＝旧串语义）。
/// 离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaConfig {
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    pub bblen: F64Param,
    pub hmalen: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkHmaConfig {
    pub fn pairs(&self) -> Vec<(f64, usize, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ls = self.hmalen.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                for &l in &ls {
                    out.push((s, b.round() as usize, l.round() as usize));
                }
            }
        }
        out
    }
}

impl BbrkHmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_hma.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_hma.bblen", 2.0, 2000.0)?;
        self.hmalen.validate_range("bbrk_hma.hmalen", 2.0, 2000.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_hma 参数组合为空".into()));
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
        let c: BbrkHmaConfig = toml::from_str("sigma = 2.5\nhmalen = 100.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 100)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkHmaConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            hmalen: F64Param::Range(RangeParam {
                start: 50.0,
                end: 100.0,
                step: 50.0,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
