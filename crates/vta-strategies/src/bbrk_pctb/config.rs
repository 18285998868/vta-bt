use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+%b离场(@2)：进场与 BBRK 相同（BB20σ突破），
    /// 离场改 %b 阈值（多头 %b < exitb 即走，空头 %b > 1−exitb 即走；
    /// %b=(close−lower)/(upper−lower)，上轨=1/下轨=0/中线=0.5）。
    /// exitb=0.5 恒等于中线离场（内置健全性检查，与 BBRK 逐位一致）。
    /// σ × exitb 笛卡尔积。
    pub struct BbrkPctbConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_pctb.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_pctb.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// 离场缓冲(价格差)。
        exitb: F64Param as f64, key "exitb", label "bbrk_pctb.exitb", range (0.0, 1.0);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), exitb.to_string()], text);
    filter true;
    empty "bbrk_pctb 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkPctbConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            exitb: F64Param::Range(RangeParam {
                start: 0.2,
                end: 0.5,
                step: 0.3,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 0.2),
                (2.0, 20, 0.5),
                (2.5, 20, 0.2),
                (2.5, 20, 0.5)
            ]
        );
    }
}
