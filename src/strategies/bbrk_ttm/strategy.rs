use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkTtmConfig;

/// Keltner 基准（与 `keltner` 策略同口径：EMA20 + k·ATR14）。
const KC_PERIOD: usize = 20;
const ATR_LEN: usize = 14;

/// BBRK+TTM Squeeze：squeeze_on ⟺ 上轨BB < 上轨KC **且** 下轨BB > 下轨KC；
/// flat + close突破 **且** 过去 n 根存在 squeeze_on → Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。任一输入 NaN → 不开仓（P7）。
pub struct BbrkTtmStrategy {
    pairs: Vec<(f64, usize, f64, usize)>,
}

impl BbrkTtmStrategy {
    /// 由 `BbrkTtmConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkTtmConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。`sqz[i]` 为调用方算好的当根 squeeze_on。
#[must_use]
pub fn bbrk_ttm_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    sqz: &[bool],
    lookback: usize,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(sqz.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            let brk_up = close[i] > upper[i];
            let brk_dn = close[i] < lower[i];
            if !brk_up && !brk_dn {
                continue;
            }
            // “存在”：过去 lookback 根（含当根）任一 squeeze_on；窗口不足拦掉。
            if lookback == 0 || i + 1 < lookback {
                continue;
            }
            if !sqz[(i + 1 - lookback)..=i].iter().any(|&s| s) {
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
    bblen: usize,
    kc_mult: f64,
    lookback: usize,
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
    let basis = match ema(&EmaInput::from_slice(
        close,
        EmaParams {
            period: Some(KC_PERIOD),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(ATR_LEN),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let n = close.len();
    let mut sqz = vec![false; n];
    for i in 0..n
        .min(bb.upper_band.len())
        .min(basis.len())
        .min(atr_vals.len())
    {
        let (up_b, lo_b, em, a) = (bb.upper_band[i], bb.lower_band[i], basis[i], atr_vals[i]);
        if ![up_b, lo_b, em, a].iter().all(|v| v.is_finite()) || a <= 0.0 {
            continue;
        }
        sqz[i] = up_b < em + kc_mult * a && lo_b > em - kc_mult * a;
    }
    Ok(bbrk_ttm_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &sqz,
        lookback,
    ))
}

impl Strategy for BbrkTtmStrategy {
    fn name(&self) -> &'static str {
        "BBRK_TTM"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, k, n) = self.pairs[idx];
        format!("sigma={s},bblen={b},kc={k},ttmn={n}")
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
        let (s, b, k, n) = self.pairs[param_idx];
        compute(high, low, close, s, b, k, n)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_recent_squeeze_breakout_buys() {
        // 当根突破 + 2根前存在 squeeze → Buy（“存在”口径，释放后突破也抓）。
        let close = vec![100.0, 100.0, 100.0, 111.0];
        let lower = vec![90.0; 4];
        let mid = vec![100.0; 4];
        let upper = vec![110.0; 4];
        let sqz = vec![false, true, false, false];
        let sig = bbrk_ttm_signals(&close, &lower, &mid, &upper, &sqz, 5);
        // 窗口不足5根(i+1<5) → Hold。补到5根重测：
        let close5 = vec![100.0, 100.0, 100.0, 100.0, 111.0];
        let lower5 = vec![90.0; 5];
        let mid5 = vec![100.0; 5];
        let upper5 = vec![110.0; 5];
        let sqz5 = vec![false, true, false, false, false];
        let sig5 = bbrk_ttm_signals(&close5, &lower5, &mid5, &upper5, &sqz5, 5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
        assert_eq!(sig5[4], Signal::Buy);
    }

    #[test]
    fn test_no_recent_squeeze_blocked() {
        // 窗内无 squeeze → 拦住。
        let close = vec![100.0, 100.0, 100.0, 100.0, 111.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sqz = vec![false; 5];
        let sig = bbrk_ttm_signals(&close, &lower, &mid, &upper, &sqz, 5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkTtmStrategy {
            pairs: vec![(2.5, 20, 1.5, 5)],
        };
        assert_eq!(s.name(), "BBRK_TTM");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,kc=1.5,ttmn=5");
    }
}
