use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BB 均值回归(官方 `BBMeanReversion` 口径): BB(20,σ)+RSI(14)，
    /// 碰下沿且 RSI<0.30 做多，碰上沿且 RSI>0.70 做空，回到中线平仓。
    /// σ 变；RSI 阈值固定(官方值)。
    pub struct BBMRConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbmr.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbmr.bblen", range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbmr 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BBMRConfig {
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
