use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// HMA方向变种5（试点，单参数）：HMA之上碰上轨**或**碰下轨都做多，
    /// HMA之下碰下轨**或**碰上轨都做空（V1+V2并集）；离场统一中线。
    /// 网格：σ × bblen × hmalen 全 Single（试点单组）。
    pub struct BbrkHmaV5Config;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_hma_v5.sigma", range (0.1, 10.0);
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_hma_v5.bblen", range (2, 2000);
        /// HMA regime 周期。
        hmalen: PeriodParam as usize, key "hmalen", label "bbrk_hma_v5.hmalen", range (2, 2000);
    }
    expand triples;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), hmalen.to_string()], text);
    filter true;
    empty "bbrk_hma_v5 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_single() {
        let c = BbrkHmaV5Config {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            hmalen: PeriodParam::Single(450),
        };
        assert_eq!(c.triples(), vec![(2.5, 20, 450)]);
    }
}
