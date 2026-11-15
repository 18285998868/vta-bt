use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+TTM fire+动量版（书中口径改编）：挤压检测与 v1 逐位一致
    /// （BB inside KC，EMA20+ATR14 KC，离场中线），只改入口——
    /// fire首根（当根squeeze_off且前根squeeze_on）＋ 动量直方图同向
    /// （Donchian20中轴+SMA20→delta→linreg20平滑，多头hist>0且rising）
    /// 才进。动量周期20书中默认，预注册固定不调参。
    /// σ × k 笛卡尔积（k{1,1.5,2,2.5}与v1同口径可比）。
    pub struct BbrkTtmFireConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_ttm_fire.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_ttm_fire.bblen", default "crate::config::default_bblen";
        /// Keltner 通道系数。
        kc: F64Param as f64, key "kc", label "bbrk_ttm_fire.kc", range (0.5, 5.0);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), kc.to_string()], text);
    filter true;
    empty "bbrk_ttm_fire 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkTtmFireConfig {
            sigma: F64Param::Single(2.5),
            kc: F64Param::Range(RangeParam {
                start: 1.0,
                end: 1.5,
                step: 0.5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 1.0), (2.5, 20, 1.5)]);
    }
}
