use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

use super::BB_MA_NAMES;

/// BBRK中轨对照（基线验证#1）：BB周期参数化 + 中轨类型对照。
/// len{5,10,15,20,25,30} × σ{2,2.5,3} × ma{sma,ema,hma,zlema}。
/// 决策与 BBRK 逐位一致（flat+收盘突破上轨多/下轨空，中线离场），
/// 只是 BB 的 period/matype 可变。sma/len20 应复刻 BBRK（ sanity 锚点）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkBbConfig {
    /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
    pub ma: Vec<String>,
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
}

impl BbrkBbConfig {
    /// 展开后的三元参数组合。
    #[must_use]
    pub fn triples(&self) -> Vec<(String, usize, f64)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.ma.len(),
            self.bblen.values().len(),
            self.sigma.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for name in &self.ma {
            for &l in &self.bblen.values() {
                for &s in &self.sigma.values() {
                    out.push((name.clone(), l, s));
                }
            }
        }
        out
    }

    /// 语义校验: 参数范围与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        // 范围校验(原在 config.rs 的 match 中, 已下沉到本类型)
        self.bblen.validate_range("bbrk_bb.bblen", 2, 2000)?;
        self.sigma.validate_range("bbrk_bb.sigma", 0.1, 10.0)?;

        for name in &self.ma {
            if !BB_MA_NAMES.contains(&name.as_str()) {
                return Err(crate::error::AppError::Msg(format!(
                    "bbrk_bb 未知均线 {name:?}，可选 {BB_MA_NAMES:?}"
                )));
            }
        }
        if self.triples().is_empty() {
            return Err(crate::error::AppError::Msg("bbrk_bb 参数组合为空".into()));
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
    fn triples_cartesian() {
        let c = BbrkBbConfig {
            ma: vec!["sma".into()],
            bblen: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            sigma: F64Param::Single(2.5),
        };
        assert_eq!(
            c.triples(),
            vec![("sma".to_string(), 5, 2.5), ("sma".to_string(), 10, 2.5)]
        );
    }

    #[test]
    fn unknown_name_rejected() {
        let c = BbrkBbConfig {
            ma: vec!["nope".into()],
            bblen: PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
        };
        assert!(c.validate().is_err()); // 原 validate_names, 已并入 validate
    }
}
