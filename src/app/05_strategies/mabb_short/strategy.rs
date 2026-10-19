use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::moving_averages::ma::{MaData, ma};
use vector_ta::indicators::stddev::{StdDevData, StdDevInput, StdDevParams, stddev};

use crate::strategies::{Signal, Strategy};

use super::config::MabbShortConfig;

/// 单均线BB空头：close<下轨做空，绝不做多；离场＝追踪止损或中线止盈先到为准。
/// 追踪：extreme为进场后close最低，trail＝extreme＋k·ATR(当根)，
/// high≥trail平仓；keff在进场根确定（stopk=0或ATR无效则追踪禁用）。
pub struct MabbShortStrategy {
    quints: Vec<(String, usize, f64, f64, usize)>,
}

impl MabbShortStrategy {
    /// 由 [`MabbShortConfig`] 构造; 参数网格由该配置展开。
    pub fn new(config: MabbShortConfig) -> Self {
        Self {
            quints: config.quints(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[allow(clippy::too_many_arguments)]
pub fn mabb_short_signals(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    atr_vals: &[f64],
    stopk: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(high.len())
        .min(low.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(atr_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    let mut extreme = 0.0;
    let mut keff = 0.0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
                extreme = close[i];
                let ae = atr_vals[i];
                keff = if stopk > 0.0 && ae.is_finite() && ae > 0.0 {
                    stopk
                } else {
                    0.0
                };
            }
        } else {
            if keff > 0.0 {
                let a = atr_vals[i];
                if a.is_finite() && high[i] >= extreme + keff * a {
                    sig[i] = Signal::Close;
                    pos = 0;
                    continue;
                }
            }
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] < extreme {
                extreme = close[i];
            }
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

/// 中轨＋波动率：mid＝MA(maname,len)，sd＝std(close,len,nbdev=1)，轨＝mid±σ·sd。
pub fn mabb_bands_short(
    close: &[f64],
    maname: &str,
    len: usize,
    sigma: f64,
) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let mid = ma(maname, MaData::Slice(close), len).ok()?;
    let sd = stddev(&StdDevInput {
        data: StdDevData::Slice(close),
        params: StdDevParams {
            period: Some(len),
            nbdev: Some(1.0),
        },
    })
    .ok()?
    .values;
    if mid.len() != close.len() || sd.len() != close.len() {
        return None;
    }
    let lower: Vec<f64> = mid
        .iter()
        .zip(sd.iter())
        .map(|(m, s)| m - sigma * s)
        .collect();
    let upper: Vec<f64> = mid
        .iter()
        .zip(sd.iter())
        .map(|(m, s)| m + sigma * s)
        .collect();
    Some((lower, mid, upper))
}

/// 中轨＋原始sd（batch缓存用，与mabb_bands_short同源，避免σ=1重构的浮点误差）。
fn mabb_core_short(close: &[f64], maname: &str, len: usize) -> Option<(Vec<f64>, Vec<f64>)> {
    let mid = ma(maname, MaData::Slice(close), len).ok()?;
    let sd = stddev(&StdDevInput {
        data: StdDevData::Slice(close),
        params: StdDevParams {
            period: Some(len),
            nbdev: Some(1.0),
        },
    })
    .ok()?
    .values;
    if mid.len() != close.len() || sd.len() != close.len() {
        return None;
    }
    Some((mid, sd))
}

pub fn mabb_atr_short(high: &[f64], low: &[f64], close: &[f64], atrlen: usize) -> Vec<f64> {
    if atrlen == 0 {
        return vec![f64::NAN; close.len()];
    }
    match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(atrlen),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => vec![f64::NAN; close.len()],
    }
}

#[allow(clippy::too_many_arguments)]
fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    maname: &str,
    len: usize,
    sigma: f64,
    stopk: f64,
    atrlen: usize,
) -> crate::error::Result<Vec<Signal>> {
    let (lower, mid, upper) = match mabb_bands_short(close, maname, len, sigma) {
        Some(v) => v,
        None => return Ok(vec![Signal::Hold; close.len()]),
    };
    let av = mabb_atr_short(high, low, close, atrlen);
    Ok(mabb_short_signals(
        close, high, low, &lower, &mid, &upper, &av, stopk,
    ))
}

impl Strategy for MabbShortStrategy {
    fn name(&self) -> &'static str {
        "MABB_SHORT"
    }

    fn param_count(&self) -> usize {
        self.quints.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (m, l, s, k, a) = &self.quints[idx];
        format!("ma={m},len={l},sigma={s},stopk={k},atrlen={a}")
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
        let (m, l, s, k, a) = &self.quints[param_idx];
        compute(high, low, close, m, *l, *s, *k, *a)
    }

    /// 批量版：按(ma,len)分组逐组算，组内复用mid/sd并行跑信号环，组间释放。
    fn generate_signals_batch(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        time: &[i64],
        range: std::ops::Range<usize>,
    ) -> crate::error::Result<Vec<Vec<Signal>>> {
        use rayon::prelude::*;
        use std::collections::{HashMap, HashSet};
        let idxs: Vec<usize> = range.collect();
        let mut alens: HashSet<usize> = HashSet::new();
        for &i in &idxs {
            alens.insert(self.quints[i].4);
        }
        let mut atr_cache: HashMap<usize, Vec<f64>> = HashMap::new();
        for a in &alens {
            atr_cache.insert(*a, mabb_atr_short(high, low, close, *a));
        }
        let mut groups: HashMap<(String, usize), Vec<usize>> = HashMap::new();
        for &i in &idxs {
            let (m, l, _, _, _) = &self.quints[i];
            groups.entry((m.clone(), *l)).or_default().push(i);
        }
        let mut keys: Vec<(String, usize)> = groups.keys().cloned().collect();
        keys.sort();
        let mut at: HashMap<usize, usize> = HashMap::new();
        for (p, &i) in idxs.iter().enumerate() {
            at.insert(i, p);
        }
        let mut out: Vec<Vec<Signal>> = vec![Vec::new(); idxs.len()];
        for (m, l) in &keys {
            let members = groups.get(&(m.clone(), *l)).cloned().unwrap_or_default();
            let (mi, sd) = match mabb_core_short(close, m, *l) {
                Some(v) => v,
                None => {
                    for &i in &members {
                        out[at[&i]] = vec![Signal::Hold; close.len()];
                    }
                    continue;
                }
            };
            let done: Vec<(usize, Vec<Signal>)> = members
                .into_par_iter()
                .map(|i| {
                    let (_, _, s, k, a) = &self.quints[i];
                    let lower: Vec<f64> = mi
                        .iter()
                        .zip(sd.iter())
                        .map(|(mm, ss)| mm - s * ss)
                        .collect();
                    let upper: Vec<f64> = mi
                        .iter()
                        .zip(sd.iter())
                        .map(|(mm, ss)| mm + s * ss)
                        .collect();
                    let av = atr_cache
                        .get(a)
                        .cloned()
                        .unwrap_or(vec![f64::NAN; close.len()]);
                    let _ = (open, volume, time);
                    (
                        i,
                        mabb_short_signals(close, high, low, &lower, &mi, &upper, &av, *k),
                    )
                })
                .collect();
            for (i, sig) in done {
                out[at[&i]] = sig;
            }
        }
        Ok(out)
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
    fn test_mid_only_control_and_never_buys() {
        let (lower, mid, upper) = bands(3);
        let sig = mabb_short_signals(
            &[101.0, 89.0, 101.0],
            &[101.0, 89.0, 101.0],
            &[101.0, 89.0, 101.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            0.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Close);
        let sig2 = mabb_short_signals(
            &[101.0, 112.0, 113.0],
            &[101.0, 112.0, 113.0],
            &[101.0, 112.0, 113.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            0.0,
        );
        assert!(sig2.iter().all(|&s| s != Signal::Buy));
    }

    #[test]
    fn test_trail_and_ratchet() {
        // entry89，trail=89+10=99：high100触发，早于中线。
        let (lower, mid, upper) = bands(3);
        let sig = mabb_short_signals(
            &[101.0, 89.0, 95.0],
            &[101.0, 89.0, 100.0],
            &[101.0, 89.0, 95.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Close);
        // 新低86抬trail至96；反抽95不触发，收<mid持有。
        let sig2 = mabb_short_signals(
            &[101.0, 89.0, 86.0, 95.0],
            &[101.0, 89.0, 86.0, 95.0],
            &[101.0, 89.0, 86.0, 95.0],
            &bands(4).0,
            &bands(4).1,
            &bands(4).2,
            &[5.0; 4],
            2.0,
        );
        assert_eq!(sig2[2], Signal::Hold);
        assert_eq!(sig2[3], Signal::Hold);
    }

    #[test]
    fn test_batch_matches_single() {
        let n = 60;
        let close: Vec<f64> = (0..n)
            .map(|i| 100.0 + 10.0 * ((i as f64) / 5.0).sin() - i as f64 * 0.2)
            .collect();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let time: Vec<i64> = (0..n as i64).collect();
        let s = MabbShortStrategy {
            quints: vec![
                ("sma".to_string(), 10, 2.0, 0.0, 0),
                ("sma".to_string(), 10, 2.0, 2.0, 14),
            ],
        };
        let batch = s
            .generate_signals_batch(&close, &high, &low, &close, &close, &time, 0..2)
            .unwrap();
        assert_eq!(batch.len(), 2);
        for (i, b) in batch.iter().enumerate() {
            let single = s
                .generate_signals_full(&close, &high, &low, &close, &close, &time, i)
                .unwrap();
            assert_eq!(*b, single, "param {i} batch与single不一致");
        }
    }

    #[test]
    fn test_name_and_params() {
        let s = MabbShortStrategy {
            quints: vec![("ema".to_string(), 20, 2.5, 0.0, 0)],
        };
        assert_eq!(s.name(), "MABB_SHORT");
        assert_eq!(s.param_str(0), "ma=ema,len=20,sigma=2.5,stopk=0,atrlen=0");
    }
}
