use serde::Deserialize;

use crate::config::F64Param;

/// HMA方向变种3止损闩锁版（试点，单参数）：入口同V3，下触逆势多加闩锁+止损，
/// 上触顺势多保持中线离场。网格：σ × bblen × stopk × atrlen 全 Single。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkHmaV3SlConfig {
    pub sigma: F64Param,
    pub bblen: F64Param,
    pub stopk: F64Param,
    pub atrlen: F64Param,
}

impl BbrkHmaV3SlConfig {
    pub fn quads(&self) -> Vec<(f64, usize, f64, usize)> {
        let mut out = Vec::new();
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &k in &self.stopk.values() {
                    for &a in &self.atrlen.values() {
                        out.push((s, b.round() as usize, k, a.round() as usize));
                    }
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
    fn quads_single() {
        let c = BbrkHmaV3SlConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            stopk: F64Param::Single(2.0),
            atrlen: F64Param::Single(50.0),
        };
        assert_eq!(c.quads(), vec![(2.5, 20, 2.0, 50)]);
    }
}
