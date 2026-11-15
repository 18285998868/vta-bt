use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+量能门（入场验证#3，用户设计）：
    /// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
    /// 进场仍 BB突破 + 方向一致（与STR/REX同口径）。
    /// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
    /// 离场保持中线。σ × len × N × m 四笛卡尔。
    pub struct BbrkVolConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_vol.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义，封存数据即len20）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_vol.bblen",
            range (2, 2000), default "crate::config::default_bblen";
        /// 量能均值窗口(bar 数)。
        voln: PeriodParam as usize, key "voln", label "bbrk_vol.voln", range (2, 500);
        /// 量能放大倍数阈值。
        volmult: F64Param as f64, key "volmult", label "bbrk_vol.volmult", range (0.5, 30.0);
    }
    expand pairs;
    code |_text| code::packed(&[
        (code::scaled(sigma, 1000.0), 14),
        (bblen as u64, 12),
        (voln as u64, 9),
        (code::scaled(volmult, 1000.0), 15),
    ]);
    filter true;
    empty "bbrk_vol 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkVolConfig = toml::from_str("sigma = 2.5\nvoln = 10\nvolmult = 1.5").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10, 1.5)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkVolConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            voln: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            volmult: F64Param::Single(1.5),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 1.5), (2.5, 20, 10, 1.5)]);
    }
}
