use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// BBRK+K线强度过滤（入场验证#1）：进场仍 BB20σ突破，叠加当根K线
    /// 实体占比 `|close−open|/(high−low) >= thr` 且方向一致
    /// （多头要求 close>open，空头 close<open）。
    /// 与已证伪的方向过滤（SMA_CD，恒等变换）同源可比：若保留率>95%
    /// 即第三次恒等陷阱，直接判死。σ × thr 笛卡尔积，thr 取 {0.3,0.5,0.7}。
    /// 离场保持中线（隔离进场效应）。
    pub struct BbrkStrConfig;
    fields {
        /// 布林带标准差倍数(通道宽度)。
        sigma: F64Param as f64, key "sigma", label "bbrk_str.sigma", range (0.1, 10.0);
        /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
        bblen: PeriodParam as usize, key "bblen", label "bbrk_str.bblen", range (2, 2000), default "crate::config::default_bblen";
        /// K 线实体比例阈值。
        body: F64Param as f64, key "body", label "bbrk_str.body", range (0.0, 1.0);
    }
    expand pairs;
    code |text| code::sigma_chain(sigma, &[bblen.to_string(), body.to_string()], text);
    filter true;
    empty "bbrk_str 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = BbrkStrConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            body: F64Param::Range(RangeParam {
                start: 0.3,
                end: 0.5,
                step: 0.2,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 0.3),
                (2.0, 20, 0.5),
                (2.5, 20, 0.3),
                (2.5, 20, 0.5)
            ]
        );
    }
}
