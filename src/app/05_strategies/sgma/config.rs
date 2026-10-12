use serde::Deserialize;

use crate::config::PeriodParam;

/// SGMA: Savitzky-Golay 平滑（官方 `sgf` endpoint smoother）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SgmaConfig {
    pub period: PeriodParam,
    #[serde(default)]
    pub poly_order: Option<PeriodParam>,
}

impl SgmaConfig {
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let rp = self.period.to_range();
        let periods: Vec<usize> = (rp.start..=rp.end).step_by(rp.step).collect();
        let orders: Vec<usize> = match &self.poly_order {
            None => vec![2],
            Some(o) => {
                let r = o.to_range();
                (r.start..=r.end).step_by(r.step).collect()
            }
        };
        let mut out = Vec::new();
        for &p in &periods {
            for &o in &orders {
                out.push((p, o));
            }
        }
        out
    }
}
