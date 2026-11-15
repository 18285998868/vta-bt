use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// HMA方向变种2（试点，单参数）：HMA之下，碰下轨**或**碰上轨都做空；
    /// HMA之上保持原规则（上破做多）。离场统一中线（顺势进场的中线离场
    /// 恰是逆势进场的均值回归目标，故不另设离场以隔离入口变量）。
    /// 网格：σ × bblen × hmalen 全 Single（试点单组）。
    pub struct BbrkHmaV2Config;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_hma_v2.sigma", range (0.1, 10.0);
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_hma_v2.bblen", range (2, 2000);
        /// HMA regime 周期。
        hmalen: PeriodParam as usize, key "hmalen", label "bbrk_hma_v2.hmalen", range (2, 2000);
    }
    expand triples;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), hmalen.to_string()], text);
    filter true;
    empty "bbrk_hma_v2 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV2Config {
            sigma: F64Param::Single(3.5),
            bblen: PeriodParam::Single(25),
            hmalen: PeriodParam::Single(450),
        };
        assert_eq!(c.triples(), vec![(3.5, 25, 450)]);
    }
}
