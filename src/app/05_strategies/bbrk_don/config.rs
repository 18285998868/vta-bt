use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+Donchian极值离场(@2)：进场与 BBRK 相同（BB突破，纯突破无方向门），
/// 离场改过去 N 根极值（多头 close 跌破近N根最低low即走，空头镜像，
/// 均不含当根，Donchian 经典口径）。中线不再离场。
/// BB周期bblen（缺省20＝旧串语义）。
/// σ × len × N 笛卡尔积；N 取整使用。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkDonConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: F64Param,
    /// 离场计数(bar 数)。
    pub exitn: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkDonConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, usize, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ns = self.exitn.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &b in &bs {
                for &n in &ns {
                    out.push((s, b.round() as usize, n.round() as usize));
                }
            }
        }
        out
    }
}

impl BbrkDonConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_don.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_don.bblen", 2.0, 2000.0)?;
        self.exitn.validate_range("bbrk_don.exitn", 2.0, 100.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_don 参数组合为空".into()));
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
        let c: BbrkDonConfig = toml::from_str("sigma = 2.5\nexitn = 10.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkDonConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: F64Param::Single(20.0),
            exitn: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 1.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 20, 2), (2.0, 20, 3), (2.5, 20, 2), (2.5, 20, 3)]
        );
    }
}
