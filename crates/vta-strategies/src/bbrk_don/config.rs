use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+Donchian极值离场(@2)：进场与 BBRK 相同（BB突破，纯突破无方向门），
    /// 离场改过去 N 根极值（多头 close 跌破近N根最低low即走，空头镜像，
    /// 均不含当根，Donchian 经典口径）。中线不再离场。
    /// BB周期bblen（缺省20＝旧串语义）。
    /// σ × len × N 笛卡尔积；N 取整使用。
    pub struct BbrkDonConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_don.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_don.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// 离场计数(bar 数)。
        exitn: PeriodParam as usize, key "exitn", label "bbrk_don.exitn", range (2, 100);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), exitn.to_string()], text);
    filter true;
    empty "bbrk_don 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkDonConfig = toml::from_str("sigma = 2.5\nexitn = 10").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkDonConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            exitn: PeriodParam::Range(RangeParam {
                start: 2,
                end: 3,
                step: 1,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.0, 20, 2), (2.0, 20, 3), (2.5, 20, 2), (2.5, 20, 3)]
        );
    }
}
