use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BB 均值回归(官方 `BBMeanReversion` 口径): BB(20,σ)+RSI(14)，
/// 碰下沿且 RSI<0.30 做多，碰上沿且 RSI>0.70 做空，回到中线平仓。
/// σ 变；RSI 阈值固定(官方值)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBMRConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: crate::config::PeriodParam,
}

fn default_bblen() -> crate::config::PeriodParam {
    crate::config::PeriodParam::Single(20)
}

impl BBMRConfig {
    /// 展开后的参数组合(σ × BB 周期)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let cap = crate::strategies::grid_prealloc(&[ss.len(), bs.len()]);
        let mut out = Vec::with_capacity(cap);
        for &s in &ss {
            for &b in &bs {
                out.push((s, b));
            }
        }
        out
    }
}

impl BBMRConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbmr.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbmr.bblen", 2, 2000)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbmr 参数组合为空".into()));
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
    fn values_expand() {
        let c = BBMRConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.0, 20), (2.5, 20), (3.0, 20)]);
    }
}
