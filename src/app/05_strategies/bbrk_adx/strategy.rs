use vector_ta::indicators::adx::{AdxInput, AdxParams, adx};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkAdxConfig;

/// BBRK+ADX: flat + close>upper && adx>=thr → Buy；
/// flat + close<lower && adx>=thr → Sell；
/// long + close<=mid → Close；short + close>=mid → Close(中线离场不过滤，
/// 避免震荡中无法离场)。BB 或 ADX 为 NaN 时不开仓(NaN→Hold，P7)。
pub struct BbrkAdxStrategy {
    pairs: Vec<(f64, usize, usize, f64)>,
}

impl BbrkAdxStrategy {
    /// 由 [`BbrkAdxConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkAdxConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn bbrk_adx_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    adx_vals: &[f64],
    adx_thr: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(adx_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        let c = close[i];
        let (lo, m, up, a) = (lower[i], mid[i], upper[i], adx_vals[i]);
        if pos == 0 {
            if ![c, lo, m, up, a].iter().all(|v| v.is_finite()) {
                continue;
            }
            if a < adx_thr {
                continue;
            }
            if c > up {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if c < lo {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if ![c, m].iter().all(|v| v.is_finite()) {
                continue;
            }
            if c <= m {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if ![c, m].iter().all(|v| v.is_finite()) {
                continue;
            }
            if c >= m {
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
    adxlen: usize,
    adx_thr: f64,
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
    let adx_vals = match adx(&AdxInput::from_slices(
        high,
        low,
        close,
        AdxParams {
            period: Some(adxlen),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    Ok(bbrk_adx_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &adx_vals,
        adx_thr,
    ))
}

impl Strategy for BbrkAdxStrategy {
    fn name(&self) -> &'static str {
        "BBRK_ADX"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, l, a, t) = self.pairs[idx];
        format!("sigma={s},len={l},adxlen={a},adx={t}")
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
        let (s, l, a, t) = self.pairs[param_idx];
        compute(high, low, close, s, l, a, t)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_break_upper_with_strong_adx_buys() {
        // lower=90, mid=100, upper=110, adx=30>=20: 111 Buy → 100 Close。
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let adx_vals = vec![30.0; 5];
        let sig = bbrk_adx_signals(&close, &lower, &mid, &upper, &adx_vals, 20.0);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_weak_adx_blocks_entry() {
        // 同样突破但 adx=10<20 → 全 Hold(过滤生效，非恒等变换 P6)。
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let adx_vals = vec![10.0; 5];
        let sig = bbrk_adx_signals(&close, &lower, &mid, &upper, &adx_vals, 20.0);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_nan_adx_blocks_entry_but_close_still_works() {
        // 持仓后 ADX 变 NaN：离场只看中线，不被 ADX 卡住。
        let close = vec![111.0, 108.0, 100.0];
        let lower = vec![90.0; 3];
        let mid = vec![100.0; 3];
        let upper = vec![110.0; 3];
        let adx_vals = vec![30.0, f64::NAN, f64::NAN];
        let sig = bbrk_adx_signals(&close, &lower, &mid, &upper, &adx_vals, 20.0);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkAdxStrategy {
            pairs: vec![(2.5, 20, 14, 20.0)],
        };
        assert_eq!(s.name(), "BBRK_ADX");
        assert_eq!(s.param_str(0), "sigma=2.5,len=20,adxlen=14,adx=20");
    }
}
