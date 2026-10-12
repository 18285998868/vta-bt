use serde::Deserialize;

use crate::config::PeriodParam;

/// ATMA: up/down 双周期笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtmaConfig {
    pub up: PeriodParam,
    pub down: PeriodParam,
}

impl AtmaConfig {
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let ru = self.up.to_range();
        let rd = self.down.to_range();
        let ups: Vec<usize> = (ru.start..=ru.end).step_by(ru.step).collect();
        let downs: Vec<usize> = (rd.start..=rd.end).step_by(rd.step).collect();
        let mut out = Vec::new();
        for &u in &ups {
            for &d in &downs {
                out.push((u, d));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = AtmaConfig {
            up: PeriodParam::Single(10),
            down: PeriodParam::Range(RangeParam {
                start: 20,
                end: 40,
                step: 20,
            }),
        };
        assert_eq!(c.pairs(), vec![(10, 20), (10, 40)]);
    }
}
