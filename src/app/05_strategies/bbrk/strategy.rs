use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BBRKConfig;

/// BBRK 布林趋势突破(WikiFX-EA口径): flat + close>upper → Buy;
/// flat + close<lower → Sell; long + close<=mid → Close;
/// short + close>=mid → Close(中线离场)。NaN→Hold。
/// 口径声明: EA用 Close[1] 算带(不含当根), 此处用当根收盘带 + delay1 次根执行,
/// 同bar带内含当根收盘属 contemporaneous 可交易, 与 ATRBand 同类处理。
pub struct BBRKStrategy {
    sigmas: Vec<f64>,
}

impl BBRKStrategy {
    pub fn new(config: BBRKConfig) -> Self {
        Self {
            sigmas: config.values(),
        }
    }
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_signals(close: &[f64], lower: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        let vs = [close[i], lower[i], mid[i], upper[i]];
        if vs.iter().any(|v| !v.is_finite()) {
            continue;
        }
        if pos == 0 {
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
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

impl Strategy for BBRKStrategy {
    fn name(&self) -> &'static str {
        "BBRK"
    }

    fn param_count(&self) -> usize {
        self.sigmas.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("sigma={}", self.sigmas[idx])
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        self.compute(close, self.sigmas[param_idx])
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

impl BBRKStrategy {
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
        Ok(bbrk_signals(
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

    #[test]
    fn test_break_upper_buys_mid_closes() {
        // lower=90, mid=100, upper=110: 111 Buy → 100 Close。
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sig = bbrk_signals(&close, &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_break_lower_sells_mid_closes() {
        let close = vec![95.0, 89.0, 92.0, 100.0, 101.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sig = bbrk_signals(&close, &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_touch_equals_hold_nan_hold() {
        // 相等不触发(严格穿越); NaN 全 Hold。
        let close = vec![110.0, 100.0];
        let lower = vec![90.0, 90.0];
        let mid = vec![100.0, 100.0];
        let upper = vec![110.0, 110.0];
        let sig = bbrk_signals(&close, &lower, &mid, &upper);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
        let nan = vec![f64::NAN, f64::NAN];
        let sig2 = bbrk_signals(&nan, &nan, &nan, &nan);
        assert!(sig2.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BBRKStrategy { sigmas: vec![2.0] };
        assert_eq!(s.name(), "BBRK");
        assert_eq!(s.param_str(0), "sigma=2");
    }
}
