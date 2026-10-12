use serde::Deserialize;

/// MACD信号线穿越：显式三元组列表(避免笛卡尔误伤出fast>=slow非法组合)。
/// 试点锁定标准组(12,26,9)+短周期组(5,35,5)，N=组数。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacdConfig {
    pub groups: Vec<[usize; 3]>,
}

impl MacdConfig {
    pub fn triples(&self) -> Vec<(usize, usize, usize)> {
        self.groups.iter().map(|g| (g[0], g[1], g[2])).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_passthrough() {
        let c = MacdConfig {
            groups: vec![[12, 26, 9], [5, 35, 5]],
        };
        assert_eq!(c.triples(), vec![(12, 26, 9), (5, 35, 5)]);
    }
}
