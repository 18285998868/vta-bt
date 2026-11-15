use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+SMA regime门（EMA基线版：BB中轨ema，门线仍SMA(L)，其余与 bbrk_sma 逐位一致）：
    /// close > SMA(L) 只允许多头（空头突破拦掉），close < SMA(L) 只允许空头；
    /// close == SMA(L) 不开仓。L{50,100,200,400} × σ3档 = 12组+BBRK对照。
    /// 离场保持中线。SMA(L) 手算（与 BB 中线 SMA20 同口径），NaN/预热不足 → 拦。
    pub struct BbrkSmaEmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_sma_ema.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_sma_ema.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// SMA regime 周期。
        smareg: PeriodParam as usize, key "smareg", label "bbrk_sma_ema.smareg", range (2, 2000);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), smareg.to_string()], text);
    filter true;
    empty "bbrk_sma_ema 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkSmaEmaConfig {
            sigma: F64Param::Single(2.5),
            smareg: PeriodParam::Range(RangeParam {
                start: 50,
                end: 100,
                step: 50,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 50), (2.5, 20, 100)]);
    }
}
