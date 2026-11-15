use crate::{ParamSet, Signal, Strategy};

use super::config::KeltnerMidConfig;

/// 肯特纳通道中线离场(研究口径, 只做多)。
///
/// 状态机与 `vbt-肯特纳通道突破.py` 的 `entries/exits` 等价:
/// - 空仓且 `close > mid + k·ATR` → `Buy`;
/// - 持多且 `close < mid` → `Close`(严格小于; 等号不离场);
/// - 任一路径的指标无效 → `Hold`。
///
/// 出场是 `Close` 而非反手, 故本策略不产出 `Sell`。
pub struct KeltnerMidStrategy {
    triples: Vec<(usize, f64, usize)>,
}

impl KeltnerMidStrategy {
    /// 由 `KeltnerMidConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: KeltnerMidConfig) -> Self {
        Self {
            triples: config.triples(),
        }
    }
}

/// Wilder ATR: `TR = max(H-L, |H-C_prev|, |L-C_prev|)`, ATR 为
/// `ewm(alpha=1/period, adjust=False, min_periods=period)`。
///
/// 与 pandas 口径的两处关键对齐(研究脚本 `atr_series` 的 wilder 分支):
/// 1. `TR[0]` 是 **NaN**(`C_prev` 不存在, `np.maximum` 传播 NaN), 不是 `H-L`;
/// 2. 递归种子是 **第一个有效 TR**, 且 `min_periods=period` 让首个有效 ATR
///    落在下标 `period`(0-based), 而不是 `period-1`。
fn wilder_atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let n = high.len().min(low.len()).min(close.len());
    let mut tr = vec![f64::NAN; n];
    for i in 1..n {
        let prev = close[i - 1];
        if !(high[i].is_finite() && low[i].is_finite() && prev.is_finite()) {
            continue;
        }
        tr[i] = (high[i] - low[i])
            .max((high[i] - prev).abs())
            .max((low[i] - prev).abs());
    }
    crate::shared::ewm_adjust_false(&tr, 1.0 / period as f64, period)
}

/// 纯决策函数(可手算单测): 只做多的通道突破/中线离场。
///
/// **水平触发**而不是策略内维护持仓状态: 研究脚本
/// `vbt-肯特纳通道突破.py` 的 `state` 由逐日 `entries/exits` 事件 ffill
/// 得到, 起点重置为空仓; 若策略自己记住 `trade_start` 之前的多头,
/// 起点后的首次上轨突破会被错误吞掉。引擎的目标状态机会把重复
/// `Buy`/`Close` 折叠成实际成交。
#[must_use]
pub fn keltner_mid_signals(close: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 0..n {
        if !close[i].is_finite() {
            continue;
        }
        // 出场优先(研究 build_weights 同一根先置 1 再置 0)。
        if mid[i].is_finite() && close[i] < mid[i] {
            sig[i] = Signal::Close;
        } else if upper[i].is_finite() && close[i] > upper[i] {
            sig[i] = Signal::Buy;
        }
    }
    sig
}

impl Strategy for KeltnerMidStrategy {
    fn name(&self) -> &'static str {
        "KELTNER_MID"
    }

    fn param_count(&self) -> usize {
        self.triples.len()
    }

    fn param_set(&self, idx: usize) -> ParamSet {
        KeltnerMidConfig::param_set(&self.triples[idx])
    }

    fn generate_signals_full(
        &self,
        bars: crate::Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let (n, k, atrlen) = self.triples[param_idx];
        let close = bars.close();
        let mid = crate::shared::ewm_adjust_false(close, 2.0 / (n as f64 + 1.0), n);
        let atr = wilder_atr(bars.high(), bars.low(), close, atrlen);
        let len = close.len().min(mid.len()).min(atr.len());
        let mut upper = vec![f64::NAN; len];
        for i in 0..len {
            if mid[i].is_finite() && atr[i].is_finite() {
                upper[i] = mid[i] + k * atr[i];
            }
        }
        Ok(keltner_mid_signals(close, &mid, &upper))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{F64Param, PeriodParam};

    fn cfg_one(n: usize) -> KeltnerMidConfig {
        KeltnerMidConfig {
            n: PeriodParam::Single(n),
            k: F64Param::Single(2.0),
            atrlen: PeriodParam::Single(10),
        }
    }

    #[test]
    fn atr_first_valid_index_is_period() {
        // TR[0] 为 NaN ⇒ 首 10 个有效 TR 落在下标 1..=10, ATR[10] 首个有效。
        let close: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let atr = wilder_atr(&close, &close, &close, 10);
        assert!(atr[..10].iter().all(|x| x.is_nan()));
        assert!(atr[10..].iter().all(|x| x.is_finite()));
    }

    #[test]
    fn break_upper_enters_and_mid_exit_closes() {
        // 上轨恒 110、中轨恒 100: 111 开多, 100 不离场(严格 <), 99 离场。
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sig = keltner_mid_signals(&close, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Hold, "等于中轨不离场");
        assert_eq!(sig[4], Signal::Close);
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn nan_bands_hold() {
        let close = vec![111.0, 112.0];
        let mid = vec![f64::NAN, f64::NAN];
        let upper = vec![f64::NAN, f64::NAN];
        assert!(
            keltner_mid_signals(&close, &mid, &upper)
                .iter()
                .all(|s| *s == Signal::Hold)
        );
    }

    #[test]
    fn jump_out_of_flat_range_enters_long() {
        // 横盘 60 根(ATR>0 使带与价格分离)后跳涨: 跳涨根突破上轨开多。
        let mut close: Vec<f64> = (0..60).map(|i| 100.0 + f64::from(i % 2)).collect();
        close.extend((0..60).map(|i| 130.0 + f64::from(i % 2)));
        let s = KeltnerMidStrategy::new(cfg_one(20));
        let sig = s
            .generate_signals_full(
                crate::Bars::new(&close, &close, &close, &close, &[], &[]),
                0,
            )
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn param_text_and_name() {
        let s = KeltnerMidStrategy::new(cfg_one(50));
        assert_eq!(s.name(), "KELTNER_MID");
        assert_eq!(s.param_str(0), "n=50,k=2,atrlen=10");
    }
}
