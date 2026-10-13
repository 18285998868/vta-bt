use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种5（试点，单参数）：HMA之上碰上轨**或**碰下轨都做多，
/// HMA之下碰下轨**或**碰上轨都做空（V1+V2并集）；离场统一中线。
/// 网格：σ × bblen × hmalen 全 Single（试点单组）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV5Config {
    pub sigma: F64Param,
    pub bblen: F64Param,
    pub hmalen: F64Param,
}

impl BbrkHmaV5Config {
    pub fn triples(&self) -> Vec<(f64, usize, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &g in &self.hmalen.values() {
                    out.push((s, b.round() as usize, g.round() as usize));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV5Config {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            hmalen: F64Param::Single(450.0),
        };
        assert_eq!(c.triples(), vec![(2.5, 20, 450)]);
    }
}
