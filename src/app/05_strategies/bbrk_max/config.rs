use serde::Deserialize;

use crate::config::F64Param;

use super::MA_NAMES;

/// BBRK+通用均线 regime门（入场验证#7）：12条均线谁来当 regime 线，
/// 平滑是否影响过滤效果，一轮定案。
/// close > MA(L) 只多，close < MA(L) 只空；L{100,200}；σ固定2.5
/// （SMA/HMA版已证σ只平移不改变排序，固定中间档省算力）。
/// 离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkMaxConfig {
    pub ma: Vec<String>,
    pub malen: F64Param,
}

impl BbrkMaxConfig {
    pub fn pairs(&self) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for name in &self.ma {
            for &l in &self.malen.values() {
                out.push((name.clone(), l.round() as usize));
            }
        }
        out
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        // 范围校验(原在 config.rs 的 match 中, 已下沉到本类型)
        self.malen.validate_range("bbrk_max.malen", 2.0, 2000.0)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkMaxConfig {
            ma: vec!["sma".into(), "hma".into()],
            malen: F64Param::Range(RangeParam {
                start: 100.0,
                end: 200.0,
                step: 100.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![
                ("sma".to_string(), 100),
                ("sma".to_string(), 200),
                ("hma".to_string(), 100),
                ("hma".to_string(), 200)
            ]
        );
    }

    #[test]
    fn unknown_name_rejected() {
        let c = BbrkMaxConfig {
            ma: vec!["nope".into()],
            malen: F64Param::Single(100.0),
        };
        assert!(c.validate().is_err()); // 原 validate_names, 已并入 validate
    }
}
