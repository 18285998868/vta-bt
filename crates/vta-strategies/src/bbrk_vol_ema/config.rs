use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+量能门（EMA基线版：BB中轨ema，其余与 bbrk_vol 逐位一致）：
    /// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
    /// 进场仍 BB20σ突破 + 方向一致（与STR/REX同口径）。
    /// 离场保持中线。σ × N × m 三笛卡尔。
    pub struct BbrkVolEmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_vol_ema.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_vol_ema.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// 量能均值窗口(bar 数)。
        voln: PeriodParam as usize, key "voln", label "bbrk_vol_ema.voln", range (2, 500);
        /// 量能放大倍数阈值。
        volmult: F64Param as f64, key "volmult", label "bbrk_vol_ema.volmult", range (0.5, 30.0);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (voln as u64, 9),
            (code::scaled(volmult, 1000.0), 15),
        ]);
    filter true;
    empty "bbrk_vol_ema 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkVolEmaConfig {
            sigma: F64Param::Single(2.5),
            voln: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            volmult: F64Param::Single(1.5),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 1.5), (2.5, 20, 10, 1.5)]);
    }
}
