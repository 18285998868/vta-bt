use serde::Deserialize;

use crate::config::PeriodParam;

use super::MA_NAMES;

/// BBRK+通用均线 regime门（入场验证#7）：12条均线谁来当 regime 线，
/// 平滑是否影响过滤效果，一轮定案。
/// close > MA(L) 只多，close < MA(L) 只空；L{100,200}；σ固定2.5
/// （SMA/HMA版已证σ只平移不改变排序，固定中间档省算力）。
/// 离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkMaxConfig {
    /// 均线类型名列表(取值见对应策略的 `*_MA_NAMES`)。
    pub ma: Vec<String>,
    /// regime 均线周期。
    pub malen: PeriodParam,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkMaxConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(String, usize, usize)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.ma.len(),
            self.malen.values().len(),
            self.bblen.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for name in &self.ma {
            for &l in &self.malen.values() {
                for &b in &self.bblen.values() {
                    out.push((name.clone(), l, b));
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
        self.malen.validate_range("bbrk_max.malen", 2, 2000)?;

        for name in &self.ma {
            if !MA_NAMES.contains(&name.as_str()) {
                return Err(crate::error::AppError::Msg(format!(
                    "bbrk_max 未知均线 {name:?}，可选 {MA_NAMES:?}"
                )));
            }
        }
        if self.pairs().is_empty() {
            return Err(crate::error::AppError::Msg("bbrk_max 参数组合为空".into()));
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
    fn pairs_cartesian() {
        let c = BbrkMaxConfig {
            ma: vec!["sma".into(), "hma".into()],
            malen: PeriodParam::Range(RangeParam {
                start: 100,
                end: 200,
                step: 100,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                ("sma".to_string(), 100, 20),
                ("sma".to_string(), 200, 20),
                ("hma".to_string(), 100, 20),
                ("hma".to_string(), 200, 20)
            ]
        );
    }

    #[test]
    fn unknown_name_rejected() {
        let c = BbrkMaxConfig {
            ma: vec!["nope".into()],
            malen: PeriodParam::Single(100),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert!(c.validate().is_err()); // 原 validate_names, 已并入 validate
    }
}
