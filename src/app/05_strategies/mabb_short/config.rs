use serde::Deserialize;

use crate::config::F64Param;

use super::MABB_MA_NAMES;

/// 单均线BB突破空头（标准差过滤变种）：close<下轨做空；离场＝追踪止损＋中线止盈；
/// stopk=0时为纯中线对照组。网格：ma × len × σ × stopk × atrlen。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MabbShortConfig {
    pub ma: Vec<String>,
    pub bblen: F64Param,
    pub sigma: F64Param,
    pub stopk: F64Param,
    pub atrlen: F64Param,
}

impl MabbShortConfig {
    pub fn quints(&self) -> Vec<(String, usize, f64, f64, usize)> {
        let mut out = Vec::new();
        for name in &self.ma {
            for &l in &self.bblen.values() {
                for &s in &self.sigma.values() {
                    for &k in &self.stopk.values() {
                        if k == 0.0 {
                            out.push((name.clone(), l.round() as usize, s, 0.0, 0));
                        } else {
                            for &a in &self.atrlen.values() {
                                out.push((
                                    name.clone(),
                                    l.round() as usize,
                                    s,
                                    k,
                                    a.round() as usize,
                                ));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        // 范围校验(原在 config.rs 的 match 中, 已下沉到本类型)
        self.sigma.validate_range("mabb_short.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("mabb_short.bblen", 2.0, 2000.0)?;
        self.stopk.validate_range("mabb_short.stopk", 0.0, 10.0)?;
        self.atrlen
            .validate_range("mabb_short.atrlen", 0.0, 500.0)?;

        for name in &self.ma {
            if !MABB_MA_NAMES.contains(&name.as_str()) {
                return Err(crate::error::AppError::Msg(format!(
                    "mabb_short 未知均线 {name:?}，可选 {MABB_MA_NAMES:?}"
                )));
            }
        }
        if self.quints().is_empty() {
            return Err(crate::error::AppError::Msg(
                "mabb_short 参数组合为空".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quints_dedupes_zero_stopk() {
        let c = MabbShortConfig {
            ma: vec!["ema".into()],
            bblen: F64Param::Single(20.0),
            sigma: F64Param::Single(2.5),
            stopk: F64Param::Single(0.0),
            atrlen: F64Param::Single(14.0),
        };
        assert_eq!(c.quints(), vec![("ema".to_string(), 20, 2.5, 0.0, 0)]);
    }
}
