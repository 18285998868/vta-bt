use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkCrossConfig;

/// BBRK+前根确认（前根位置对照实验）：flat + 当根 `close>upper`
/// **且** 前根收盘在轨内（含触线，即 `lower[i-1]<=close[i-1]<=upper[i-1]`）
/// → Buy；flat + 当根 `close<lower` 且前根在轨内 → Sell；
/// 持仓后中线离场（与 BBRK 相同）。
/// 首根（无前根）/前根缺失或 NaN → 不开仓（P7）。
pub struct BbrkCrossStrategy {
    sigmas: Vec<f64>,
}

impl BbrkCrossStrategy {
    pub fn new(config: BbrkCrossConfig) -> Self {
        Self {
            sigmas: config.values(),
        }
    }
}

/// 纯决策函数(可手算单测；与 `bbrk_signals` 同形，仅多前根确认)。
pub fn bbrk_cross_signals(close: &[f64], lower: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], mid[i], upper[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            // 前根确认：首根无前值、前根任一输入非有限 → Hold（P7）。
            if i == 0 {
                continue;
            }
            if ![close[i - 1], lower[i - 1], upper[i - 1]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            // 前根在轨内（含触线；触线当根按 BBRK 本就不触发，故触线算“内”）。
            if close[i - 1] < lower[i - 1] || close[i - 1] > upper[i - 1] {
                continue;
            }
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] <= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if close[i] >= mid[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

impl Strategy for BbrkCrossStrategy {
    fn name(&self) -> &'static str {
        "BBRK_CROSS"
    }

    fn param_count(&self) -> usize {
        self.sigmas.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("sigma={}", self.sigmas[idx])
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        _high: &[f64],
        _low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        self.compute(close, self.sigmas[param_idx])
    }
}

impl BbrkCrossStrategy {
    fn compute(&self, close: &[f64], sigma: f64) -> crate::error::Result<Vec<Signal>> {
        let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
            close,
            BollingerBandsParams {
                period: Some(20),
                devup: Some(sigma),
                devdn: Some(sigma),
                matype: Some("sma".to_string()),
                devtype: Some(0),
            },
        )) {
            Ok(v) => v,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(bbrk_cross_signals(
            close,
            &bb.lower_band,
            &bb.middle_band,
            &bb.upper_band,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_prev_inside_then_break_buys() {
        // 前根100在轨内 → 当根111突破 Buy；首根恒 Hold（无前值）。
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_cross_signals(&[100.0, 111.0], &lower, &mid, &upper);
        assert_eq!(sig[0], Signal::Hold);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_prev_outside_blocks_entry() {
        // 前根111已在轨外 → 当根112不再进（BBRK 此处会 Buy）。
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_cross_signals(&[111.0, 112.0], &lower, &mid, &upper);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_prev_touch_counts_as_inside() {
        // 前根110触线算轨内 → 当根111突破 Buy。
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_cross_signals(&[110.0, 111.0], &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_short_mirror() {
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_cross_signals(&[100.0, 89.0], &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Sell);
        let sig2 = bbrk_cross_signals(&[89.0, 88.0], &lower, &mid, &upper);
        assert!(sig2.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_exit_same_as_bbrk() {
        let (lower, mid, upper) = bands(4);
        let sig = bbrk_cross_signals(&[100.0, 111.0, 108.0, 100.0], &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_nan_hold() {
        let (lower, mid, upper) = bands(2);
        let nan = vec![f64::NAN, f64::NAN];
        let sig = bbrk_cross_signals(&nan, &nan, &nan, &nan);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
        // 前根 NaN 也不开仓（P7）。
        let sig2 = bbrk_cross_signals(&[f64::NAN, 111.0], &lower, &mid, &upper);
        assert!(sig2.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkCrossStrategy { sigmas: vec![2.5] };
        assert_eq!(s.name(), "BBRK_CROSS");
        assert_eq!(s.param_str(0), "sigma=2.5");
    }
}
