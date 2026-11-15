use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// HMA方向变种4（试点，单参数）：碰上轨**或**碰下轨都做空（无门，
    /// HMA不参与）；离场统一中线。σ × bblen 网格。
    pub struct BbrkHmaV4Config;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_hma_v4.sigma", range (0.1, 10.0);
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_hma_v4.bblen", range (2, 2000);
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk_hma_v4 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_single() {
        let c = BbrkHmaV4Config {
            sigma: F64Param::Single(3.5),
            bblen: PeriodParam::Single(25),
        };
        assert_eq!(c.pairs(), vec![(3.5, 25)]);
    }
}
