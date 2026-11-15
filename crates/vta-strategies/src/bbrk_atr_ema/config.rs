use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+ATR扩张门（EMA基线版：BB中轨ema，其余与 bbrk_atr 逐位一致）：进场仍 BB20σ突破 + 方向一致，
    /// 叠加 `ATR5/ATR20 > thr`（Wilder口径，长度5/20固定）。
    /// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × thr 笛卡尔积。
    /// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
    pub struct BbrkAtrEmaConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_atr_ema.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_atr_ema.bblen",
            range (2, 2000), default "crate::config::default_bblen";
        /// ATR 突破阈值(倍数)。
        atrthr: F64Param as f64, key "atrthr", label "bbrk_atr_ema.atrthr", range (0.1, 5.0);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), atrthr.to_string()], text);
    filter true;
    empty "bbrk_atr_ema 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAtrEmaConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 0.8),
                (2.0, 20, 1.0),
                (2.5, 20, 0.8),
                (2.5, 20, 1.0)
            ]
        );
    }
}
