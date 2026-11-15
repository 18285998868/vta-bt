use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// Bollinger Method II 趋势跟随（官方 Method II 手册口径，2026-10-11）：
    /// BB(20, sma, σ) + %b>0.8 且 MFI(10)>80 进多头（强价格+强指标确认），
    /// 镜像 %b<0.2 且 MFI(10)<20 进空头；MFI 周期取带宽周期之半（官方规则）。
    /// 离场中线（原文为修正抛物线/对侧带触碰，为隔离进场差异此处统一中线，
    /// 见 strategy.rs）。σ{2.0,2.5,3.0} = 3组+BBRK锚点。
    pub struct BbrkM2Config;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_m2.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_m2.bblen", range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk_m2 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BbrkM2Config {
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
