use serde::Deserialize;

use crate::config::PeriodParam;

/// 海龟 Donchian 突破。entry/exit 独立笛卡尔(经典 20/10 与 55/20 皆在其中)。
/// ATR 仓位管理 vta 固定名义表达不了, 只复现信号层(差异见研究报告)。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurtleConfig {
    pub entry: PeriodParam,
    pub exit: PeriodParam,
}

impl TurtleConfig {
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let e = self.entry.to_range();
        let x = self.exit.to_range();
        let es: Vec<usize> = (e.start..=e.end).step_by(e.step).collect();
        let xs: Vec<usize> = (x.start..=x.end).step_by(x.step).collect();
        let mut out = Vec::new();
        for &a in &es {
            for &b in &xs {
                out.push((a, b));
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
    fn pairs_cartesian_entry_exit() {
        let c = TurtleConfig {
            entry: PeriodParam::Range(RangeParam {
                start: 20,
                end: 55,
                step: 35,
            }),
            exit: PeriodParam::Range(RangeParam {
                start: 10,
                end: 20,
                step: 10,
            }),
        };
        assert_eq!(c.pairs(), vec![(20, 10), (20, 20), (55, 10), (55, 20)]);
    }
}
