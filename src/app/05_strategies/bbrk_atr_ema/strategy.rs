use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkAtrEmaConfig;

/// ATR短长窗口（固定，经典短长配）。
const ATR_SHORT: usize = 5;
const ATR_LONG: usize = 20;

/// BBRK+ATR扩张门：flat + close突破 **且** 方向一致 **且**
/// `ATR5/ATR20 > thr` → Buy/Sell；持仓后中线离场（与 BBRK 相同）。
/// ATR NaN（预热）/ATR20非正 → 不开仓（P7）。
pub struct BbrkAtrEmaStrategy {
    pairs: Vec<(f64, f64)>,
}

impl BbrkAtrEmaStrategy {
    pub fn new(config: BbrkAtrEmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；8参为OHLC+三轨+双ATR+阈值的最小完备集)。
#[allow(clippy::too_many_arguments)]
pub fn bbrk_atr_ema_signals(
    open: &[f64],
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    atr5: &[f64],
    atr20: &[f64],
    thr: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(atr5.len())
        .min(atr20.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            let (o, c) = (open[i], close[i]);
            if ![o, c, lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            let brk_up = c > upper[i];
            let brk_dn = c < lower[i];
            if !brk_up && !brk_dn {
                continue;
            }
            if (brk_up && c <= o) || (brk_dn && c >= o) {
                continue;
            }
            let (a5, a20) = (atr5[i], atr20[i]);
            if ![a5, a20].iter().all(|v| v.is_finite()) || a20 <= 0.0 {
                continue;
            }
            if a5 / a20 <= thr {
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
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    thr: f64,
) -> crate::error::Result<Vec<Signal>> {
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
    let a5 = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(ATR_SHORT),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let a20 = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(ATR_LONG),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    Ok(bbrk_atr_ema_signals(
        open,
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &a5,
        &a20,
        thr,
    ))
}

impl Strategy for BbrkAtrEmaStrategy {
    fn name(&self) -> &'static str {
        "BBRK_ATR_EMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, t) = self.pairs[idx];
        format!("sigma={s},atrthr={t}")
    }

    fn generate_signals(
        &self,
        open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        // 无 high/low 时 ATR 无从算起 → 用 close 充当（ATR=0 → 门全拦，P7）。
        let (s, t) = self.pairs[param_idx];
        compute(open, close, close, close, s, t)
    }

    fn generate_signals_full(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, t) = self.pairs[param_idx];
        compute(open, high, low, close, s, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_expanding_breakout_buys() {
        // ATR5/ATR20 = 3/2 = 1.5 > 1.0 → Buy。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sig = bbrk_atr_ema_signals(
            &[100.0, 100.0],
            &[101.0, 112.0],
            &lower,
            &mid,
            &upper,
            &[2.0, 3.0],
            &[2.0, 2.0],
            1.0,
        );
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_contracting_breakout_blocked() {
        // 1.5/2 = 0.75 ≤ 1.0 → Hold。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sig = bbrk_atr_ema_signals(
            &[100.0, 100.0],
            &[101.0, 112.0],
            &lower,
            &mid,
            &upper,
            &[2.0, 1.5],
            &[2.0, 2.0],
            1.0,
        );
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_nan_atr_holds() {
        let (lower, mid, upper) = (vec![90.0], vec![100.0], vec![110.0]);
        let sig = bbrk_atr_ema_signals(
            &[100.0],
            &[112.0],
            &lower,
            &mid,
            &upper,
            &[f64::NAN],
            &[2.0],
            1.0,
        );
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkAtrEmaStrategy {
            pairs: vec![(2.5, 1.0)],
        };
        assert_eq!(s.name(), "BBRK_ATR_EMA");
        assert_eq!(s.param_str(0), "sigma=2.5,atrthr=1");
    }
}
