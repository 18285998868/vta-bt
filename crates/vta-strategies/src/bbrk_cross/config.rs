use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+前根确认（前根位置对照实验，2026-10-11）：决策与 `bbrk_signals`
    /// 逐位一致，仅多一条件——进场要求前一根收盘在轨内（含触线）。
    /// 首根/前根缺失或 NaN → 不开仓（P7）。
    /// σ{2.0,2.5,3.0} = 3组+BBRK对照（IS单窗108单元预注册）。
    pub struct BbrkCrossConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_cross.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_cross.bblen", range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk_cross 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BbrkCrossConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.0, 20), (2.5, 20), (3.0, 20)]);
    }
}
