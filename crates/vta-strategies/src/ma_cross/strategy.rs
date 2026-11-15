use crate::{ParamSet, Signal, Strategy};

use super::config::MaCrossConfig;

/// 双均线交叉(研究口径, 只做多)。
///
/// 与研究脚本 `vbt-双均线交叉.py` 的 `cross_signals` + 状态机逐 bar 同义:
/// - `fast > slow` 且此前不在多头 → `Buy`;
/// - `fast < slow` 且当前持多 → `Close`(平仓, 不反手);
/// - 任一条均线无效 → `Hold`。
///
/// 三条均线均为**自实现**, 目的是与 pandas 口径逐位对齐(详见
/// `ma_series` 的注释): `sma` = `rolling(n).mean()`, `ema` =
/// `ewm(span=n, adjust=False).min_periods=n`, `hma` = 嵌套 WMA
/// (`n//2` 与 `round(sqrt(n))` 的半周期/根周期口径)。
pub struct MaCrossStrategy {
    triples: Vec<(String, usize, usize)>,
}

impl MaCrossStrategy {
    /// 由 `MaCrossConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: MaCrossConfig) -> Self {
        Self {
            triples: config.triples(),
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

/// pandas `ewm(span=span, adjust=False, min_periods=min_periods)`。
///
/// `adjust=False` 的口径是**从第一个有效值起递归**, 而 `min_periods` 只把
/// 前 `min_periods-1` 个输出置 `NaN` —— 不是先取 SMA 做种子。两者在
/// `min_periods == span` 时的前缀数值**不同**, 用后者会在研究复现里引入
/// 几十根 bar 的均线偏移。实现见
/// [`ewm_adjust_false`](crate::shared::ewm_adjust_false)。
fn ema(values: &[f64], span: usize) -> Vec<f64> {
    crate::shared::ewm_adjust_false(values, 2.0 / (span as f64 + 1.0), span)
}

/// 线性加权移动平均(最近一根权重最大, 权重 `1..=n`); 窗口必须全有效。
fn wma(values: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; values.len()];
    if n == 0 || n > values.len() {
        return out;
    }
    let total = (n * (n + 1) / 2) as f64;
    for i in n - 1..values.len() {
        let window = &values[i + 1 - n..=i];
        if window.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let mut acc = 0.0;
        for (k, v) in window.iter().enumerate() {
            acc += v * (k + 1) as f64;
        }
        out[i] = acc / total;
    }
    out
}

/// Hull 移动平均: `WMA(2·WMA(n/2) − WMA(n), round(√n))`。
///
/// 半周期用**整除**(`n//2`), 根周期用 **round**(不是 ceil), 与研究脚本
/// `hma()` 逐字一致。
fn hma(values: &[f64], n: usize) -> Vec<f64> {
    let half = (n / 2).max(1);
    let root = ((n as f64).sqrt().round() as usize).max(1);
    let fast = wma(values, half);
    let slow = wma(values, n);
    let raw: Vec<f64> = fast
        .iter()
        .zip(slow.iter())
        .map(|(&f, &s)| {
            if f.is_finite() && s.is_finite() {
                2.0 * f - s
            } else {
                f64::NAN
            }
        })
        .collect();
    wma(&raw, root)
}

/// 按名单计算整条均线; 未知名单返回 `None`(配置期已校验, 这里只兜底)。
#[must_use]
pub fn ma_series(values: &[f64], kind: &str, n: usize) -> Option<Vec<f64>> {
    match kind {
        "sma" => Some(sma(values, n)),
        "ema" => Some(ema(values, n)),
        "hma" => Some(hma(values, n)),
        _ => None,
    }
}

/// 纯决策函数(可手算单测): 双均线交叉的**只做多**状态机。
///
/// 与研究脚本的 `entries/exits` 布尔序列等价: 首个双有效 bar 按当下
/// `fast > slow` 决定是否进场, 之后只在状态翻转处给信号; 出场是 `Close`。
#[must_use]
pub fn ma_cross_signals(fast: &[f64], slow: &[f64]) -> Vec<Signal> {
    let n = fast.len().min(slow.len());
    let mut sig = vec![Signal::Hold; n];
    let mut long = false;
    for i in 0..n {
        if !fast[i].is_finite() || !slow[i].is_finite() {
            continue;
        }
        let above = fast[i] > slow[i];
        if above && !long {
            sig[i] = Signal::Buy;
            long = true;
        } else if !above && long {
            sig[i] = Signal::Close;
            long = false;
        }
    }
    sig
}

impl Strategy for MaCrossStrategy {
    fn name(&self) -> &'static str {
        "MA_CROSS"
    }

    fn param_count(&self) -> usize {
        self.triples.len()
    }

    fn param_set(&self, idx: usize) -> ParamSet {
        MaCrossConfig::param_set(&self.triples[idx])
    }

    fn generate_signals_full(
        &self,
        bars: crate::Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let (kind, fast_p, slow_p) = &self.triples[param_idx];
        let (Some(fast), Some(slow)) = (
            ma_series(bars.close(), kind, *fast_p),
            ma_series(bars.close(), kind, *slow_p),
        ) else {
            return Err(crate::param_skipped(format!("MA_CROSS: 未知均线 {kind:?}")));
        };
        Ok(ma_cross_signals(&fast, &slow))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PeriodParam;

    fn cfg_one(kind: &str, fast_p: usize, slow_p: usize) -> MaCrossConfig {
        MaCrossConfig {
            ma: vec![kind.into()],
            fast: PeriodParam::Single(fast_p),
            slow: PeriodParam::Single(slow_p),
        }
    }

    #[test]
    fn sma_matches_hand_computation() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        let s = sma(&v, 3);
        assert!(s[0].is_nan() && s[1].is_nan());
        assert!((s[2] - 2.0).abs() < 1e-12);
        assert!((s[4] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn ema_recurses_from_first_value_with_min_periods_mask() {
        // span=3 → alpha=0.5; 递归种子 = 第一根收盘。
        let v = [10.0, 20.0, 30.0, 40.0];
        let e = crate::shared::ewm_adjust_false(&v, 0.5, 3);
        assert!(e[0].is_nan() && e[1].is_nan());
        // y1 = 20*0.5 + 10*0.5 = 15; y2 = 30*0.5 + 15*0.5 = 22.5;
        // y3 = 40*0.5 + 22.5*0.5 = 31.25
        assert!((e[2] - 22.5).abs() < 1e-12);
        assert!((e[3] - 31.25).abs() < 1e-12);
    }

    #[test]
    fn hma_warmup_is_n_plus_root_minus_two() {
        // n=9 → root=3, 首个有效下标 = 9 + 3 - 2 = 10。
        let v: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let h = hma(&v, 9);
        assert!(h[..10].iter().all(|x| x.is_nan()));
        assert!(h[10..].iter().all(|x| x.is_finite()));
    }

    #[test]
    fn monotonic_up_enters_long_once_without_sell() {
        let close: Vec<f64> = (0..300).map(|i| 100.0 + i as f64).collect();
        let s = MaCrossStrategy::new(cfg_one("sma", 20, 60));
        let sig = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &close, &[], &[]), 0)
            .unwrap();
        assert_eq!(sig.len(), 300);
        assert_eq!(sig.iter().filter(|x| **x == Signal::Buy).count(), 1);
        assert!(!sig.contains(&Signal::Sell));
        assert!(!sig.contains(&Signal::Close));
    }

    #[test]
    fn down_cross_closes_instead_of_reversing() {
        let mut close: Vec<f64> = (0..200).map(|i| 100.0 + i as f64).collect();
        close.extend((0..200).map(|i| 300.0 - i as f64));
        let s = MaCrossStrategy::new(cfg_one("ema", 5, 20));
        let sig = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &close, &[], &[]), 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
        assert!(sig.contains(&Signal::Close));
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn param_text_and_name() {
        let s = MaCrossStrategy::new(cfg_one("hma", 50, 200));
        assert_eq!(s.name(), "MA_CROSS");
        assert_eq!(s.param_str(0), "ma=hma,fast=50,slow=200");
    }
}
