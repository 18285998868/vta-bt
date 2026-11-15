use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 正统布林形态对照（Bollinger 2001 第二章 W 底 / M 顶 + %b 背离，
    /// 2026-10-11）：BB(20, sma, σ) 经典口径；形态回看窗 W 预注册固定为 10
    /// 根（见 `ORTH_W`，不调参，防 P2），σ{2.0,2.5,3.0} = 3组+BBRK对照
    /// （IS单窗108单元预注册）。
    pub struct BbrkOrthConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_orth.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_orth.bblen", range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk_orth 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BbrkOrthConfig {
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
