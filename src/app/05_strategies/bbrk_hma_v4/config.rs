use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种4（试点，单参数）：碰上轨**或**碰下轨都做空（无门，
/// HMA不参与）；离场统一中线。σ × bblen 网格。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV4Config {
    pub sigma: F64Param,
    pub bblen: F64Param,
}

impl BbrkHmaV4Config {
    pub fn pairs(&self) -> Vec<(f64, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                out.push((s, b.round() as usize));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_single() {
        let c = BbrkHmaV4Config {
            sigma: F64Param::Single(3.5),
            bblen: F64Param::Single(25.0),
        };
        assert_eq!(c.pairs(), vec![(3.5, 25)]);
    }
}
