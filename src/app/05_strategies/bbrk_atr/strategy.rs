use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkAtrConfig;

/// BBRK+ATR扩张门：flat + close突破 **且** 方向一致 **且**
/// `ATR(atr1)/ATR(atr2) > thr` → Buy/Sell；持仓后中线离场（与 BBRK 相同）。
/// ATR NaN（预热）/长窗ATR非正 → 不开仓（P7）。
pub struct BbrkAtrStrategy {
    pairs: Vec<(f64, usize, usize, usize, f64)>,
}

impl BbrkAtrStrategy {
    pub fn new(config: BbrkAtrConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；8参为OHLC+三轨+双ATR+阈值的最小完备集)。
#[allow(clippy::too_many_arguments)]
pub fn bbrk_atr_signals(
    open: &[f64],
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    atr_short: &[f64],
    atr_long: &[f64],
    thr: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(atr_short.len())
        .min(atr_long.len());
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
            let (a1, a2) = (atr_short[i], atr_long[i]);
            if ![a1, a2].iter().all(|v| v.is_finite()) || a2 <= 0.0 {
                continue;
            }
            if a1 / a2 <= thr {
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

#[allow(clippy::too_many_arguments)]
fn compute(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    bblen: usize,
    atr1: usize,
    atr2: usize,
    thr: f64,
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
    let a1 = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(atr1) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let a2 = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(atr2) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    Ok(bbrk_atr_signals(
        open,
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &a1,
        &a2,
        thr,
    ))
}

impl Strategy for BbrkAtrStrategy {
    fn name(&self) -> &'static str {
        "BBRK_ATR"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, l, a1, a2, t) = self.pairs[idx];
        format!("sigma={s},bblen={l},atr1={a1},atr2={a2},atrthr={t}")
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
        let (s, l, a1, a2, t) = self.pairs[param_idx];
        compute(open, high, low, close, s, l, a1, a2, t)
    }

    /// 批量版：ATR 按窗长去重（各unique长度算一次），BB 按 (sigma, bblen)
    /// 去重，再并行跑各 (atr1, atr2, thr)。输出与逐个调逐位一致（单测锁定）。
    fn generate_signals_batch(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        range: std::ops::Range<usize>,
    ) -> crate::error::Result<Vec<Vec<Signal>>> {
        use rayon::prelude::*;
        use std::collections::HashMap;
        use vector_ta::indicators::bollinger_bands::{
            BollingerBandsInput, BollingerBandsParams, bollinger_bands,
        };
        let mut atrmap: HashMap<usize, Option<Vec<f64>>> = HashMap::new();
        for idx in range.clone() {
            let (_, _, a1, a2, _) = self.pairs[idx];
            for len in [a1, a2] {
                atrmap.entry(len).or_insert_with(|| {
                    atr(&AtrInput::from_slices(
                        high,
                        low,
                        close,
                        AtrParams { length: Some(len) },
                    ))
                    .ok()
                    .map(|a| a.values)
                });
            }
        }
        let mut bbmap: HashMap<(u64, usize), crate::strategies::CachedBands> = HashMap::new();
        for idx in range.clone() {
            let (s, l, _, _, _) = self.pairs[idx];
            bbmap.entry((s.to_bits(), l)).or_insert_with(|| {
                bollinger_bands(&BollingerBandsInput::from_slice(
                    close,
                    BollingerBandsParams {
                        period: Some(l),
                        devup: Some(s),
                        devdn: Some(s),
                        matype: Some("sma".to_string()),
                        devtype: Some(0),
                    },
                ))
                .ok()
                .map(|bb| (bb.lower_band, bb.middle_band, bb.upper_band))
            });
        }
        range
            .into_par_iter()
            .map(|idx| {
                let (s, l, a1, a2, t) = self.pairs[idx];
                match (
                    atrmap.get(&a1),
                    atrmap.get(&a2),
                    bbmap.get(&(s.to_bits(), l)),
                ) {
                    (Some(Some(short)), Some(Some(long)), Some(Some((lower, mid, upper)))) => Ok(
                        bbrk_atr_signals(open, close, lower, mid, upper, short, long, t),
                    ),
                    _ => Ok(vec![Signal::Hold; close.len()]),
                }
            })
            .collect()
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
        let sig = bbrk_atr_signals(
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
        let sig = bbrk_atr_signals(
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
        let sig = bbrk_atr_signals(
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
    fn test_batch_matches_single() {
        let n = 60;
        let base: Vec<f64> = (0..n)
            .map(|i| 100.0 + (i as f64 * 0.2).sin() * 6.0)
            .collect();
        let open = base.clone();
        let high: Vec<f64> = base.iter().map(|v| v + 2.0).collect();
        let low: Vec<f64> = base.iter().map(|v| v - 2.0).collect();
        let close: Vec<f64> = base.iter().map(|v| v + 0.5).collect();
        let time: Vec<i64> = (0..n as i64).collect();
        let st = BbrkAtrStrategy {
            pairs: vec![
                (2.5, 20, 5, 20, 1.0),
                (2.5, 20, 5, 20, 1.2),
                (2.0, 10, 5, 10, 0.8),
            ],
        };
        let batched = st
            .generate_signals_batch(&open, &high, &low, &close, &[], &time, 0..3)
            .unwrap();
        assert_eq!(batched.len(), 3);
        for (i, sig) in batched.iter().enumerate() {
            let one = st
                .generate_signals_full(&open, &high, &low, &close, &[], &time, i)
                .unwrap();
            assert_eq!(*sig, one, "param {i} 不一致");
        }
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkAtrStrategy {
            pairs: vec![(2.5, 20, 5, 20, 1.0)],
        };
        assert_eq!(s.name(), "BBRK_ATR");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,atr1=5,atr2=20,atrthr=1");
    }
}
