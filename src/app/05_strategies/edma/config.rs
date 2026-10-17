use serde::Deserialize;

use crate::config::PeriodParam;

/// EDMA: Exponential Displaced Moving Average。
/// `edma[i] = ema[i - offset]`（向后偏移 = 延迟版 EMA，无前视）。
/// period/offset 独立成范围，笛卡尔扫描。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdmaConfig {
    pub period: PeriodParam,
    #[serde(default)]
    pub offset: Option<PeriodParam>,
}

impl EdmaConfig {
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let pr = self.period.to_range();
        let periods: Vec<usize> = (pr.start..=pr.end).step_by(pr.step).collect();
        let offsets: Vec<usize> = match &self.offset {
            None => vec![0],
            Some(o) => {
                let r = o.to_range();
                (r.start..=r.end).step_by(r.step).collect()
            }
        };
        let mut out = Vec::new();
        for &p in &periods {
            for &o in &offsets {
                out.push((p, o));
            }
        }
        out
    }
}

impl EdmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = EdmaConfig {
            period: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            offset: Some(PeriodParam::Range(RangeParam {
                start: 0,
                end: 2,
                step: 2,
            })),
        };
        assert_eq!(c.pairs(), vec![(5, 0), (5, 2), (10, 0), (10, 2)]);
    }
}
