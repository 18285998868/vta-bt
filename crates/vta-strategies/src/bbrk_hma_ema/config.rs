use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+HMA regime门（入场验证#6）：SMA门嫌滞后，换 Hull 均线
    /// （平滑+低滞后，vector-ta 现成指标直接用）。
    /// close > HMA(L) 只允许多头，close < HMA(L) 只允许空头，相等不开仓。
    /// L{50,100,200,400} × σ3档 = 12组+BBRK对照，与 SMA 版逐位可比。
    /// 离场保持中线。
    pub struct BbrkHmaEmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_hma_ema.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_hma_ema.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// HMA regime 周期。
        hmalen: PeriodParam as usize, key "hmalen", label "bbrk_hma_ema.hmalen", range (2, 2000);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), hmalen.to_string()], text);
    filter true;
    empty "bbrk_hma_ema 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkHmaEmaConfig {
            sigma: F64Param::Single(2.5),
            hmalen: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
