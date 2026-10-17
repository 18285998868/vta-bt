use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaEmaConfig;

/// BBRK+HMA regime门（EMA基线版：BB中轨ema，其余与 bbrk_hma 逐位一致）：flat + close突破 **且** regime一致
/// （多头要求 close > HMA(L)，空头要求 close < HMA(L)）→ Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。
/// HMA 预热不足/NaN → 不开仓（P7）。
pub struct BbrkHmaEmaStrategy {
    pairs: Vec<(f64, usize)>,
}

impl BbrkHmaEmaStrategy {
    pub fn new(config: BbrkHmaEmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；与 `bbrk_sma_signals` 同形，regime 线换成 HMA)。
pub fn bbrk_hma_ema_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    hma_vals: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(hma_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i], hma_vals[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            if close[i] > upper[i] && close[i] > hma_vals[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] && close[i] < hma_vals[i] {
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
        } else {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(close: &[f64], sigma: f64, len: usize) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(20),
            devup: Some(sigma),
            devdn: Some(sigma),
            matype: Some("ema".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let ma = match hma(&HmaInput::from_slice(
        close,
        HmaParams { period: Some(len) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    Ok(bbrk_hma_ema_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &ma,
    ))
}

impl Strategy for BbrkHmaEmaStrategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_EMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, l) = self.pairs[idx];
        format!("sigma={s},hmalen={l}")
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
        let (s, l) = self.pairs[param_idx];
        compute(close, s, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_regime_aligned_breakout_buys() {
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let hma_vals = vec![105.0; 2];
        let sig = bbrk_hma_ema_signals(&[101.0, 112.0], &lower, &mid, &upper, &hma_vals);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_counter_regime_breakout_blocked() {
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let hma_vals = vec![115.0; 2];
        let sig = bbrk_hma_ema_signals(&[101.0, 112.0], &lower, &mid, &upper, &hma_vals);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaEmaStrategy {
            pairs: vec![(2.5, 100)],
        };
        assert_eq!(s.name(), "BBRK_HMA_EMA");
        assert_eq!(s.param_str(0), "sigma=2.5,hmalen=100");
    }
}
