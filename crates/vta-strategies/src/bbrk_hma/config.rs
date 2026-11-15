use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+HMA regime门（入场验证#6）：SMA门嫌滞后，换 Hull 均线
    /// （平滑+低滞后，vector-ta 现成指标直接用）。
    /// close > HMA(L) 只允许多头，close < HMA(L) 只允许空头，相等不开仓。
    /// L{50,100,200,400} × σ3档 = 12组+BBRK对照，与 SMA 版逐位可比。
    /// BB周期bblen（缺省20＝旧串语义）。
    /// 离场保持中线。
    pub struct BbrkHmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_hma.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_hma.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// HMA regime 周期。
        hmalen: PeriodParam as usize, key "hmalen", label "bbrk_hma.hmalen", range (2, 2000);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), hmalen.to_string()], text);
    filter true;
    empty "bbrk_hma 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkHmaConfig = toml::from_str("sigma = 2.5\nhmalen = 100").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 100)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkHmaConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            hmalen: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
