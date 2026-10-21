use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// 正统布林形态对照（Bollinger 2001 第二章 W 底 / M 顶 + %b 背离，
/// 2026-10-11）：BB(20, sma, σ) 经典口径；形态回看窗 W 预注册固定为 10
/// 根（见 `ORTH_W`，不调参，防 P2），σ{2.0,2.5,3.0} = 3组+BBRK对照
/// （IS单窗108单元预注册）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkOrthConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: crate::config::PeriodParam,
}

fn default_bblen() -> crate::config::PeriodParam {
    crate::config::PeriodParam::Single(20)
}

impl BbrkOrthConfig {
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

impl BbrkOrthConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_orth.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_orth.bblen", 2, 2000)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_orth 参数组合为空".into()));
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
        let c = BbrkOrthConfig {
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
