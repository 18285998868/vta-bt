use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+TTM Squeeze正统版（用户设计2）：布林带上下轨落在
    /// Keltner(EMA20 ± k·ATR14) 上下轨内部即 squeeze_on（BB inside KC，与书/ToS一致；
    /// 实现见 strategy.rs line144，本注释2026-10-12修正，此前写反但代码一直是对的）；
    /// 过去 n 根（含当根）存在 squeeze_on 且当根 BB 突破 → 进；离场保持中线。
    /// σ × k × n 三笛卡尔（用户指定 k{1,1.5,2,2.5}；n 取 {5,10} 紧/松两档）。
    pub struct BbrkTtmConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_ttm.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_ttm.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// Keltner 通道系数。
        kc: F64Param as f64, key "kc", label "bbrk_ttm.kc", range (0.5, 5.0);
        /// TTM 挤压回看窗口。
        ttmn: PeriodParam as usize, key "ttmn", label "bbrk_ttm.ttmn", range (2, 100);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (code::scaled(kc, 1000.0), 13),
            (ttmn as u64, 7),
        ]);
    filter true;
    empty "bbrk_ttm 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkTtmConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            kc: F64Param::Single(1.5),
            ttmn: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 1.5, 5),
                (2.0, 20, 1.5, 10),
                (2.5, 20, 1.5, 5),
                (2.5, 20, 1.5, 10)
            ]
        );
    }
}
