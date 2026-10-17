use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkSmaConfig;

/// BBRK+SMA regime门：flat + close突破 **且** regime一致
/// （多头要求 close > SMA(L)，空头要求 close < SMA(L)）→ Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。
/// SMA(L) 预热不足/NaN → 不开仓（P7）。
pub struct BbrkSmaStrategy {
    pairs: Vec<(f64, usize, usize)>,
}

impl BbrkSmaStrategy {
    pub fn new(config: BbrkSmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_sma_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    sma: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(sma.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i], sma[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            if close[i] > upper[i] && close[i] > sma[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] && close[i] < sma[i] {
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

fn rolling_sma(close: &[f64], len: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if len == 0 {
        return out;
    }
    let mut pre = vec![0.0; n + 1];
    let mut cnt = vec![0usize; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i];
        cnt[i + 1] = cnt[i];
        if close[i].is_finite() {
            pre[i + 1] += close[i];
            cnt[i + 1] += 1;
        }
    }
    for i in 0..n {
        let lo = (i + 1).saturating_sub(len);
        if i + 1 >= len && cnt[i + 1] - cnt[lo] == len {
            out[i] = (pre[i + 1] - pre[lo]) / len as f64;
        }
    }
    out
}

fn compute(
    close: &[f64],
    sigma: f64,
    bblen: usize,
    reg: usize,
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
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let sma = rolling_sma(close, reg);
    Ok(bbrk_sma_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &sma,
    ))
}

impl Strategy for BbrkSmaStrategy {
    fn name(&self) -> &'static str {
        "BBRK_SMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, l) = self.pairs[idx];
        format!("sigma={s},bblen={b},smareg={l}")
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
        let (s, b, l) = self.pairs[param_idx];
        compute(close, s, b, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_regime_aligned_breakout_buys() {
        // 突破上轨且在线上 → Buy。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sma = vec![105.0; 2];
        let sig = bbrk_sma_signals(&[101.0, 112.0], &lower, &mid, &upper, &sma);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_counter_regime_breakout_blocked() {
        // 突破上轨但在线下（close<sma）→ 拦掉。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sma = vec![115.0; 2];
        let sig = bbrk_sma_signals(&[101.0, 112.0], &lower, &mid, &upper, &sma);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_nan_sma_holds() {
        let (lower, mid, upper) = (vec![90.0], vec![100.0], vec![110.0]);
        let sma = vec![f64::NAN];
        let sig = bbrk_sma_signals(&[112.0], &lower, &mid, &upper, &sma);
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_rolling_sma_warmup() {
        // 窗内缺一根即 NaN。
        let w = rolling_sma(&[1.0, 2.0, 3.0], 3);
        assert!(w[0].is_nan() && w[1].is_nan());
        assert!((w[2] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkSmaStrategy {
            pairs: vec![(2.5, 20, 100)],
        };
        assert_eq!(s.name(), "BBRK_SMA");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,smareg=100");
    }
}
