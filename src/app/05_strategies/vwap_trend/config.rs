use serde::Deserialize;

use crate::config::F64Param;

/// VWAP顺势（Zarattini & Aziz 2023 SSRN 4631351 口径的加密适配）：
/// 线上持多、线下持空，穿越即反手。`reset` 五种session定义：
/// 0 = 过去24h滚动VWAP（不重置、日内日夜连续），1 = UTC日切重置+午夜平仓，
/// 2/3/4 = 亚(00–08UTC)/欧(08–16)/美(16–24)三分盘：VWAP在盘内累计、
/// 只在盘内持仓（出盘即平+空过盘外bar）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VwapTrendConfig {
    pub reset: F64Param,
}

impl VwapTrendConfig {
    pub fn modes(&self) -> Vec<u8> {
        self.reset
            .values()
            .iter()
            .map(|v| v.round() as u8)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn modes_round() {
        let c = VwapTrendConfig {
            reset: F64Param::Range(RangeParam {
                start: 0.0,
                end: 1.0,
                step: 1.0,
            }),
        };
        assert_eq!(c.modes(), vec![0, 1]);
    }
}
