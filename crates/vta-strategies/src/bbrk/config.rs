use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK 布林趋势突破(WikiFX-EA口径): BB(20,σ)收盘突破上轨做多、
    /// 跌破下轨做空，回到中线平仓。σ 变；周期固定20(布林经典值)。
    pub struct BBRKConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk.bblen",
            range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BBRKConfig {
            sigma: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(1.5, 20), (2.0, 20), (2.5, 20)]);
    }
}
