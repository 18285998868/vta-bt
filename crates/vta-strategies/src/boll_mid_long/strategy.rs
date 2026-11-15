use crate::{Bars, ParamSet, Signal, Strategy};

use super::config::BollMidLongConfig;

/// 布林带突破(研究口径, 只做多)。
///
/// 与 `vbt-布林带突破.py` 的 `entries = close > upper` /
/// `exits = close < mid` 逐 bar 同义:
/// - 收盘价高于上轨 `SMA(N) + k·σ(ddof=0)` → `Buy`;
/// - 收盘价低于中轨 `SMA(N)` → `Close`;
/// - 出场优先于入场(研究状态机同一根先置 1 再置 0);
/// - 指标未预热(NaN) → `Hold`。
///
/// 与 `donchian_long` / `keltner_mid` 一样是**水平触发**: 起点重置为空仓后,
/// 只要当下收盘在带外, 状态机就应给出对应目标; 引擎的目标状态机会把重复
/// `Buy`/`Close` 折叠成实际成交。策略内不再维护第二份状态 —— 否则
/// `trade_start` 之前的历史仓位会压制起点后的重新入场。
pub struct BollMidLongStrategy {
    pairs: Vec<(usize, f64)>,
}

impl BollMidLongStrategy {
    /// 由 [`BollMidLongConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BollMidLongConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 简单移动平均; 前 `n-1` 根为 `NaN`(`rolling(n, min_periods=n).mean()`)。
fn sma(values: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; values.len()];
    if n == 0 || n > values.len() {
        return out;
    }
    for i in n - 1..values.len() {
        let window = &values[i + 1 - n..=i];
        if window.iter().any(|v| !v.is_finite()) {
            continue;
        }
        out[i] = window.iter().sum::<f64>() / n as f64;
    }
    out
}

/// 总体标准差(`ddof=0`), 与研究脚本
/// `close.rolling(n, min_periods=n).std(ddof=0)` 同口径。
fn std0(values: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; values.len()];
    if n == 0 || n > values.len() {
        return out;
    }
    for i in n - 1..values.len() {
        let window = &values[i + 1 - n..=i];
        if window.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let mean = window.iter().sum::<f64>() / n as f64;
        let var = window
            .iter()
            .map(|v| {
                let d = v - mean;
                d * d
            })
            .sum::<f64>()
            / n as f64;
        out[i] = var.sqrt();
    }
    out
}

/// 纯决策函数(可手算单测): 出场优先, NaN → `Hold`。
#[must_use]
pub fn boll_mid_long_signals(close: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 0..n {
        if !close[i].is_finite() {
            continue;
        }
        if mid[i].is_finite() && close[i] < mid[i] {
            sig[i] = Signal::Close;
        } else if upper[i].is_finite() && close[i] > upper[i] {
            sig[i] = Signal::Buy;
        }
    }
    sig
}

fn signals_from_close(close: &[f64], n: usize, k: f64) -> Vec<Signal> {
    let mid = sma(close, n);
    let sd = std0(close, n);
    let len = close.len().min(mid.len()).min(sd.len());
    let mut upper = vec![f64::NAN; len];
    for i in 0..len {
        if mid[i].is_finite() && sd[i].is_finite() {
            upper[i] = mid[i] + k * sd[i];
        }
    }
    boll_mid_long_signals(close, &mid, &upper)
}

impl Strategy for BollMidLongStrategy {
    fn name(&self) -> &'static str {
        "BOLL_MID_LONG"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_set(&self, idx: usize) -> ParamSet {
        BollMidLongConfig::param_set(&self.pairs[idx])
    }

    fn generate_signals_full(
        &self,
        bars: Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let (n, k) = self.pairs[param_idx];
        Ok(signals_from_close(bars.close(), n, k))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{F64Param, PeriodParam};

    fn cfg_one(n: usize) -> BollMidLongConfig {
        BollMidLongConfig {
            n: PeriodParam::Single(n),
            k: F64Param::Single(2.0),
        }
    }

    #[test]
    fn bands_use_population_std() {
        // 窗口 [1,2,3]: mean=2, 总体方差=2/3, sd=sqrt(2/3).
        let v = [1.0, 2.0, 3.0, 4.0];
        let mid = sma(&v, 3);
        let sd = std0(&v, 3);
        assert!(mid[0].is_nan() && mid[1].is_nan());
        assert!((mid[2] - 2.0).abs() < 1e-12);
        assert!((sd[2] - (2.0_f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((mid[3] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn break_upper_enters_and_mid_exit_closes() {
        // 上轨恒 110、中轨恒 100: 111 开多, 100 不离场(严格 <), 99 离场。
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sig = boll_mid_long_signals(&close, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Hold, "等于中轨不离场");
        assert_eq!(sig[4], Signal::Close);
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn exit_wins_when_both_bands_are_breached() {
        // 病态输入(mid > upper): 出场优先, 与研究 build_weights 的同序赋值一致。
        let sig = boll_mid_long_signals(&[105.0], &[110.0], &[100.0]);
        assert_eq!(sig[0], Signal::Close);
    }

    #[test]
    fn nan_bands_hold() {
        let close = [111.0, 112.0];
        let nan = [f64::NAN, f64::NAN];
        assert!(
            boll_mid_long_signals(&close, &nan, &nan)
                .iter()
                .all(|s| *s == Signal::Hold)
        );
    }

    #[test]
    fn param_text_and_name() {
        let s = BollMidLongStrategy::new(cfg_one(50));
        assert_eq!(s.name(), "BOLL_MID_LONG");
        assert_eq!(s.param_str(0), "n=50,k=2");
    }
}
