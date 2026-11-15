use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+ATR扩张门（入场验证#4）：进场仍 BB突破 + 方向一致，
    /// 叠加 `ATR(atr1)/ATR(atr2) > thr`（Wilder口径）。
    /// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × len × atr1 × atr2 × thr 笛卡尔积，
    /// 只取 atr1<atr2（短长语义，ema_cross同口径）。
    /// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
    /// ATR短长窗（缺省5/20＝旧串语义，经典短长配）。
    /// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
    pub struct BbrkAtrConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_atr.sigma", range (0.1, 10.0);
        /// BB周期（缺省20＝旧串语义，封存数据即len20）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_atr.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// ATR短窗（缺省5＝旧串语义）。
        /// 快 ATR 周期。
        atr1: PeriodParam as usize, key "atr1", label "bbrk_atr.atr1", range (2, 500), default "crate::config::default_atr1";
        /// ATR长窗（缺省20＝旧串语义）。
        /// 慢 ATR 周期。
        atr2: PeriodParam as usize, key "atr2", label "bbrk_atr.atr2", range (2, 500), default "crate::config::default_atr2";
        /// ATR 突破阈值(倍数)。
        atrthr: F64Param as f64, key "atrthr", label "bbrk_atr.atrthr", range (0.1, 5.0);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (atr1 as u64, 9),
            (atr2 as u64, 9),
            (code::scaled(atrthr, 1000.0), 13),
        ]);
    filter (atr1 < atr2);
    empty "bbrk_atr 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_atr_defaults_to_20_5_20() {
        let c: BbrkAtrConfig = toml::from_str("sigma = 2.5\natrthr = 1.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 20, 1.0)]);
    }

    #[test]
    fn short_not_less_than_long_filtered() {
        let c: BbrkAtrConfig =
            toml::from_str("sigma = 2.5\nbblen = 20\natr1 = 10\natr2 = 10\natrthr = 1.0").unwrap();
        assert!(c.pairs().is_empty());
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAtrConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            atr1: PeriodParam::Single(5),
            atr2: PeriodParam::Single(20),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.5, 20, 5, 20, 0.8), (2.5, 20, 5, 20, 1.0)]
        );
    }
}
