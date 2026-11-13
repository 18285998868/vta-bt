use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

use super::MABB_MA_NAMES;

/// 单均线BB突破多头（标准差过滤变种）：中轨为任一单均线，
/// close>上轨做多；离场＝追踪止损＋中线止盈；stopk=0时为纯中线对照组。
/// 网格：ma × len × σ × stopk × atrlen（stopk=0时atrlen去重为单值）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MabbLongConfig {
    /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
    pub ma: Vec<String>,
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// 止损 ATR 倍数。
    pub stopk: F64Param,
    /// ATR 周期。
    pub atrlen: PeriodParam,
}

impl MabbLongConfig {
    /// 展开后的五元参数组合。
    #[must_use]
    pub fn quints(&self) -> Vec<(String, usize, f64, f64, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.ma.len(),
            self.bblen.values().len(),
            self.sigma.values().len(),
            self.stopk.values().len(),
            self.atrlen.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for name in &self.ma {
            for &l in &self.bblen.values() {
                for &s in &self.sigma.values() {
                    for &k in &self.stopk.values() {
                        if k == 0.0 {
                            out.push((name.clone(), l, s, 0.0, 0));
                        } else {
                            for &a in &self.atrlen.values() {
                                out.push((name.clone(), l, s, k, a));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// 语义校验: 参数范围与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        // 范围校验(原在 config.rs 的 match 中, 已下沉到本类型)
        self.sigma.validate_range("mabb_long.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("mabb_long.bblen", 2, 2000)?;
        self.stopk.validate_range("mabb_long.stopk", 0.0, 10.0)?;
        self.atrlen.validate_range("mabb_long.atrlen", 0, 500)?;

        for name in &self.ma {
            if !MABB_MA_NAMES.contains(&name.as_str()) {
                return Err(crate::error::AppError::Msg(format!(
                    "mabb_long 未知均线 {name:?}，可选 {MABB_MA_NAMES:?}"
                )));
            }
        }
        if self.quints().is_empty() {
            return Err(crate::error::AppError::Msg("mabb_long 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quints_dedupes_zero_stopk() {
        let c = MabbLongConfig {
            ma: vec!["sma".into()],
            bblen: PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
            stopk: F64Param::Single(0.0),
            atrlen: PeriodParam::Single(14),
        };
        assert_eq!(c.quints(), vec![("sma".to_string(), 20, 2.5, 0.0, 0)]);
    }
}
