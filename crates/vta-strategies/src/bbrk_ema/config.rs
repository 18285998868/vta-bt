use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+EMA regime门：SMA门同形，regime线换 EMA（vector-ta 现成指标）。
    /// close > EMA(L) 只允许多头，close < EMA(L) 只允许空头；
    /// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
    /// BB周期bblen（缺省20＝旧串语义）。
    /// 离场保持中线。EMA(L) NaN/预热不足 → 拦。
    pub struct BbrkEmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_ema.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_ema.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// EMA regime 周期。
        emareg: PeriodParam as usize, key "emareg", label "bbrk_ema.emareg", range (2, 2000);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), emareg.to_string()], text);
    filter true;
    empty "bbrk_ema 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkEmaConfig = toml::from_str("sigma = 2.5\nemareg = 100").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 100)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkEmaConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            emareg: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
