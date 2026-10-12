use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkBwConfig;

/// BBRK+归一化带宽阈值：flat + close突破 **且** `(upper−lower)/mid < mult·ATR14/mid`
/// → Buy/Sell；持仓后中线离场（与 BBRK 相同）。
/// ATR 非正/非有限、带宽 NaN → 不开仓（P7）。
pub struct BbrkBwStrategy {
    pairs: Vec<(f64, f64)>,
}

impl BbrkBwStrategy {
    pub fn new(config: BbrkBwConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_bw_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    atr_vals: &[f64],
    mult: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(atr_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            let (c, lo, m, up, a) = (close[i], lower[i], mid[i], upper[i], atr_vals[i]);
            if ![c, lo, m, up, a].iter().all(|v| v.is_finite()) || m <= 0.0 || up <= lo || a <= 0.0
            {
                continue;
            }
            let brk_up = c > up;
            let brk_dn = c < lo;
            if !brk_up && !brk_dn {
                continue;
            }
            if (up - lo) / m >= mult * (a / m) {
                continue;
            }
            if brk_up {
                sig[i] = Signal::Buy;
                pos = 1;
            } else {
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

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    mult: f64,
) -> crate::error::Result<Vec<Signal>> {
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
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(14) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    Ok(bbrk_bw_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &atr_vals,
        mult,
    ))
}

impl Strategy for BbrkBwStrategy {
    fn name(&self) -> &'static str {
        "BBRK_BW"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, m) = self.pairs[idx];
        format!("sigma={s},bwmult={m}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, m) = self.pairs[param_idx];
        compute(close, close, close, s, m)
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, m) = self.pairs[param_idx];
        compute(high, low, close, s, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_compressed_breakout_buys() {
        // 带宽 (110−90)/100=0.2 < 6×(2/100)=0.12？0.2>0.12 不通过。
        // 改 mult=12：0.2<0.24 → Buy。
        let close = vec![100.0, 111.0];
        let lower = vec![90.0; 2];
        let mid = vec![100.0; 2];
        let upper = vec![110.0; 2];
        let atr_vals = vec![2.0; 2];
        let sig = bbrk_bw_signals(&close, &lower, &mid, &upper, &atr_vals, 12.0);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_wide_breakout_blocked() {
        // 同上 mult=6：0.2 ≥ 0.12 → Hold。
        let close = vec![100.0, 111.0];
        let lower = vec![90.0; 2];
        let mid = vec![100.0; 2];
        let upper = vec![110.0; 2];
        let atr_vals = vec![2.0; 2];
        let sig = bbrk_bw_signals(&close, &lower, &mid, &upper, &atr_vals, 6.0);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_bad_atr_holds() {
        // ATR 非正/NaN → 不开仓。
        let close = vec![111.0];
        let lower = vec![90.0];
        let mid = vec![100.0];
        let upper = vec![110.0];
        for a in [0.0, -1.0, f64::NAN] {
            let sig = bbrk_bw_signals(&close, &lower, &mid, &upper, &[a], 12.0);
            assert_eq!(sig[0], Signal::Hold);
        }
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkBwStrategy {
            pairs: vec![(2.5, 6.0)],
        };
        assert_eq!(s.name(), "BBRK_BW");
        assert_eq!(s.param_str(0), "sigma=2.5,bwmult=6");
    }
}
