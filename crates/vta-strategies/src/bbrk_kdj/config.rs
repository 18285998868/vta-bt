use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+KDJ逆势波段（视频策略改编，独立研究`策略研究记录/KDJ-001/`）：
    /// 挤压（BB inside KC，KC=SMA20±1.5·ATR14，视频默认）＋ KDJ(25)超买超卖翻转。
    /// 多头：布防（squeeze_on ＋ close≤lower ＋ K<20）→ 触发（K>20 ＋ close>open）
    /// → Buy，止损＝触发根前推10根最低；离场：持仓见过K>80后K<80 → Close。
    /// 空头镜像。KDJ slow按库默认（映射假设已声明）。
    /// σ × 3档；KDJ周期等全固定（一次一变：只测“逆势结构”本身）。
    pub struct BbrkKdjConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_kdj.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_kdj.bblen", range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "bbrk_kdj 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BbrkKdjConfig {
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
