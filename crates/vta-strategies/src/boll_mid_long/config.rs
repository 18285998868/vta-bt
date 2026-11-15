use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 布林带突破(研究口径, 只做多): 中轨 `SMA(N)`、总体标准差(`ddof=0`),
    /// 上轨 `mid + k·σ`; 收盘突破上轨开多, 收盘跌破中轨平仓。
    ///
    /// 与内置 `bbrk` 的区别: `bbrk` 在空仓跌破下轨时做空(引擎 `long_only`
    /// 只能过滤成交, 内部状态仍会进入 short, 可能吞掉后续上轨突破),
    /// 且中线离场用 `<=`。研究脚本 `vbt-布林带突破.py` 是严格
    /// `close > upper` / `close < mid` 的 long/flat 两态, 本策略按此实现。
    pub struct BollMidLongConfig;
    fields {
        /// 布林带周期(研究口径 20 / 50)。
        n: PeriodParam as usize, key "n", label "boll_mid_long.n", range (2, 2000);
        /// 标准差倍数(研究口径 2.0 / 2.1)。
        k: F64Param as f64, key "k", label "boll_mid_long.k", range (0.1, 10.0);
    }
    expand pairs;
    code |_text| code::packed(&[(n as u64, 12), (code::scaled(k, 1000.0), 14)]);
    filter true;
    empty "boll_mid_long 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian_n_k() {
        let c = BollMidLongConfig {
            n: PeriodParam::Range(RangeParam {
                start: 20,
                end: 50,
                step: 30,
            }),
            k: F64Param::Single(2.0),
        };
        assert_eq!(c.pairs(), vec![(20, 2.0), (50, 2.0)]);
    }

    #[test]
    fn param_text_is_stable() {
        let p = BollMidLongConfig::param_set(&(20, 2.0));
        assert_eq!(p.text(), "n=20,k=2");
        assert_eq!(p.json(), r#"{"n":"20","k":"2"}"#);
        assert_eq!(
            p.code(),
            crate::params::code::packed(&[(20, 12), (2000, 14)])
        );
    }
}
