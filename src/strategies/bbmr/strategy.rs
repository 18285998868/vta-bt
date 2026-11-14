use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::rsi::{RsiInput, RsiParams, rsi};

use crate::strategies::{Signal, Strategy};

use super::config::BBMRConfig;

/// BB 均值回归(官方口径): flat + close<=lower && rsi<0.30 → Buy;
/// flat + close>=upper && rsi>0.70 → Sell;
/// long + close>=mid → Close; short + close<=mid → Close。NaN→Hold。
/// vector-ta RSI 为 0–100 经典口径(阈值 30/70，非 nautilus 自带指标的 0–1)。
pub struct BBMRStrategy {
    pairs: Vec<(f64, usize)>,
}

impl BBMRStrategy {
    /// 由 `BBMRConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BBMRConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn bbmr_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    rsi_vals: &[f64],
    buy_thr: f64,
    sell_thr: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(rsi_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        let vs = [close[i], lower[i], mid[i], upper[i], rsi_vals[i]];
        if vs.iter().any(|v| !v.is_finite()) {
            continue;
        }
        if pos == 0 {
            if close[i] <= lower[i] && rsi_vals[i] < buy_thr {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] >= upper[i] && rsi_vals[i] > sell_thr {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if close[i] <= mid[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

impl Strategy for BBMRStrategy {
    fn name(&self) -> &'static str {
        "BBMR"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("sigma={},bblen={}", self.pairs[idx].0, self.pairs[idx].1)
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        self.compute(
            bars.close(),
            self.pairs[param_idx].0,
            self.pairs[param_idx].1,
        )
    }
}

impl BBMRStrategy {
    fn compute(
        &self,
        close: &[f64],
        sigma: f64,
        bblen: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
            close,
            BollingerBandsParams {
                period: Some(bblen),
                devup: Some(sigma),
                devdn: Some(sigma),
                matype: Some("sma".to_string()),
                devtype: Some(0),
            },
        )) {
            Ok(v) => v,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        let rsi_vals = match rsi(&RsiInput::from_slice(close, RsiParams { period: Some(14) })) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        Ok(bbmr_signals(
            close,
            &bb.lower_band,
            &bb.middle_band,
            &bb.upper_band,
            &rsi_vals,
            30.0,
            70.0,
        ))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_touch_lower_buys_mid_closes() {
        // lower=90, mid=100, upper=110, rsi=20: 89 Buy → 100 Close。
        let close = vec![95.0, 89.0, 92.0, 100.0, 101.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let rsi_vals = vec![20.0; 5];
        let sig = bbmr_signals(&close, &lower, &mid, &upper, &rsi_vals, 30.0, 70.0);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_touch_upper_sells() {
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let rsi_vals = vec![80.0; 5];
        let sig = bbmr_signals(&close, &lower, &mid, &upper, &rsi_vals, 30.0, 70.0);
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BBMRStrategy {
            pairs: vec![(2.0, 20)],
        };
        assert_eq!(s.name(), "BBMR");
        assert_eq!(s.param_str(0), "sigma=2,bblen=20");
    }
}
