use crate::config::{F64Param, PeriodParam};
use crate::grid::grid_config;
use crate::params::code;

grid_config! {
    /// 肯特纳通道中线离场(研究口径, 只做多): 中轨 `EMA(close, n)`,
    /// 上轨 `mid + k·ATR(atrlen)`, 收盘突破上轨开多, 收盘跌破中轨平仓。
    ///
    /// 研究脚本 `vbt-肯特纳通道突破.py` 的 `mid_mode=ema` / `atr_mode=wilder`
    /// / `exit_mode=mid` 口径。与内置 `keltner`(EMA20/ATR14、上下轨双向穿越)
    /// 的区别: 周期可配、只在上轨进场、中线离场用严格 `<`。
    pub struct KeltnerMidConfig;
    fields {
        /// 中轨 EMA 周期(研究口径 20 / 50)。
        n: PeriodParam as usize, key "n", label "keltner_mid.n", range (2, 2000);
        /// ATR 倍数(研究口径 2.0)。
        k: F64Param as f64, key "k", label "keltner_mid.k", range (0.1, 10.0);
        /// ATR 周期(Wilder RMA, 研究口径 10)。
        atrlen: PeriodParam as usize, key "atrlen", label "keltner_mid.atrlen",
            range (1, 500);
    }
    expand triples;
    code |_text| code::packed(&[
        (n as u64, 12),
        (code::scaled(k, 1000.0), 14),
        (atrlen as u64, 9),
    ]);
    filter true;
    empty "keltner_mid 参数组合为空";
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn triples_cartesian() {
        let c = KeltnerMidConfig {
            n: PeriodParam::Range(RangeParam {
                start: 20,
                end: 50,
                step: 30,
            }),
            k: F64Param::Single(2.0),
            atrlen: PeriodParam::Single(10),
        };
        assert_eq!(c.triples(), vec![(20, 2.0, 10), (50, 2.0, 10)]);
    }

    #[test]
    fn param_text_and_code_are_stable() {
        let p = KeltnerMidConfig::param_set(&(20, 2.0, 10));
        assert_eq!(p.text(), "n=20,k=2,atrlen=10");
        assert_eq!(p.json(), r#"{"n":"20","k":"2","atrlen":"10"}"#);
        assert_eq!(
            p.code(),
            crate::params::code::packed(&[(20, 12), (2000, 14), (10, 9)])
        );
    }
}
