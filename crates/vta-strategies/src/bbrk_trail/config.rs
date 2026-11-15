use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+ATR追踪离场(@2)：进场与 BBRK 完全相同（BB突破，纯突破无方向门），
    /// 离场改用 Chandelier 式追踪（多头 close < 近N根最高high − k·ATR 即走，
    /// 空头镜像），不再碰中线就走。追踪窗N=22固定。
    /// BB周期bblen（缺省20＝旧串语义）；ATR周期atrlen（缺省14＝旧串语义）。
    /// σ × len × atrlen × k 笛卡尔积。
    pub struct BbrkTrailConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_trail.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_trail.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// ATR周期（缺省14＝旧串语义）。
        /// ATR 周期。
        atrlen: PeriodParam as usize, key "atrlen", label "bbrk_trail.atrlen", range (2, 500), default "crate::config::default_atrlen";
        /// 跟踪止损 ATR 倍数。
        trailk: F64Param as f64, key "trailk", label "bbrk_trail.trailk", range (0.1, 20.0);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (atrlen as u64, 9),
            (code::scaled(trailk, 1000.0), 15),
        ]);
    filter true;
    empty "bbrk_trail 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_atrlen_default_to_20_14() {
        let c: BbrkTrailConfig = toml::from_str("sigma = 2.5\ntrailk = 3.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 14, 3.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkTrailConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            atrlen: PeriodParam::Single(14),
            trailk: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 1.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 14, 2.0),
                (2.0, 20, 14, 3.0),
                (2.5, 20, 14, 2.0),
                (2.5, 20, 14, 3.0)
            ]
        );
    }
}
