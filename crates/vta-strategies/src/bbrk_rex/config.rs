use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+振幅扩张过滤（入场验证#2，用户设计）：
    /// 当根振幅与过去 n 根对比：`(high−low) > k × mean(high−low, n)`，
    /// 且方向一致（多头 close>open），进场仍 BB20σ突破。
    /// n 用户指定 {3,5,7,9,11,13,15}；k{1.5,2,2.5,3}（2026-10-12起参数化，
    /// 原mult=1.5固定；旧"sigma=S,rexn=N"参数串封存，新串多",rexmult=M"后缀）。
    /// BB周期bblen（2026-10-12起参数化，默认20即经典；旧串视为len20）。
    /// 与验证#1对照：方向要求相同，差异只在强度定义
    /// （根内实体占比 vs 跨根振幅扩张）。离场保持中线。
    pub struct BbrkRexConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_rex.sigma", range (0.1, 10.0);
        /// BB周期（2026-10-12起参数化；缺省20＝旧串语义，封存数据即len20）。
        /// 布林带周期。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_rex.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// 挤压计数窗口。
        rexn: PeriodParam as usize, key "rexn", label "bbrk_rex.rexn", range (2, 100);
        /// 挤压判定倍数。
        rexmult: F64Param as f64, key "rexmult", label "bbrk_rex.rexmult", range (0.5, 30.0);
    }
    expand pairs;
    code |_text| code::packed(&[
            (code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (rexn as u64, 8),
            (code::scaled(rexmult, 1000.0), 15),
        ]);
    filter true;
    empty "bbrk_rex 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkRexConfig = toml::from_str("sigma = 2.5\nrexn = 7\nrexmult = 2.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 7, 2.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkRexConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            rexn: PeriodParam::Range(RangeParam {
                start: 3,
                end: 5,
                step: 2,
            }),
            rexmult: F64Param::Single(2.0),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 3, 2.0), (2.5, 20, 5, 2.0)]);
    }
}
