use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+ADX 震荡过滤(BBRK 突破 + ADX 门限): 只在 `adx >= thr` 时允许开仓，
    /// 回到中线无条件平仓。σ × len × adxlen × thr 笛卡尔积。
    /// BB周期bblen（缺省20＝旧串语义）；ADX周期adxlen（缺省14＝旧串语义，经典值）。
    pub struct BbrkAdxConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_adx.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "len", label "bbrk_adx.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// ADX周期（缺省14＝旧串语义）。
        /// ADX 周期。
        adxlen: PeriodParam as usize, key "adxlen", label "bbrk_adx.adxlen", range (2, 200), default "crate::config::default_adxlen";
        /// ADX 强度阈值。
        adx: F64Param as f64, key "adx", label "bbrk_adx.adx", range (0.0, 100.0);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (adxlen as u64, 9),
            (code::scaled(adx, 10.0), 10),
        ]);
    filter true;
    empty "bbrk_adx 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_adxlen_default_to_20_14() {
        let c: BbrkAdxConfig = toml::from_str("sigma = 2.5\nadx = 20.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 14, 20.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAdxConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            adxlen: PeriodParam::Single(14),
            adx: F64Param::Range(RangeParam {
                start: 20.0,
                end: 25.0,
                step: 5.0,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 14, 20.0),
                (2.0, 20, 14, 25.0),
                (2.5, 20, 14, 20.0),
                (2.5, 20, 14, 25.0)
            ]
        );
    }
}
