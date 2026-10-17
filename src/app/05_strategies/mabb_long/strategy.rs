use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::moving_averages::ma::{MaData, ma};
use vector_ta::indicators::stddev::{StdDevData, StdDevInput, StdDevParams, stddev};

use crate::strategies::{Signal, Strategy};

use super::config::MabbLongConfig;

/// 单均线BB多头：close>上轨做多，绝不做空；离场＝追踪止损或中线止盈先到为准。
/// 追踪：extreme为进场后close最高，trail＝extreme−k·ATR(当根)，
/// low≤trail平仓；keff在进场根确定（stopk=0或ATR无效则追踪禁用）。
/// 中线：close≤mid平仓（止盈语义：保住剩余）。
pub struct MabbLongStrategy {
    quints: Vec<(String, usize, f64, f64, usize)>,
}

impl MabbLongStrategy {
    pub fn new(config: MabbLongConfig) -> Self {
        Self {
            quints: config.quints(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[allow(clippy::too_many_arguments)]
pub fn mabb_long_signals(
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
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
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
                if a.is_finite() && low[i] <= extreme - keff * a {
                    sig[i] = Signal::Close;
                    pos = 0;
                    continue;
                }
            }
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > extreme {
                extreme = close[i];
            }
            if close[i] <= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

/// 中轨＋波动率：mid＝MA(maname,len)，sd＝std(close,len,nbdev=1)，轨＝mid±σ·sd。
pub fn mabb_bands(
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

/// 中轨＋原始sd（batch缓存用，与mabb_bands同源，避免σ=1重构的浮点误差）。
fn mabb_core(close: &[f64], maname: &str, len: usize) -> Option<(Vec<f64>, Vec<f64>)> {
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

pub fn mabb_atr(high: &[f64], low: &[f64], close: &[f64], atrlen: usize) -> Vec<f64> {
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
    let (lower, mid, upper) = match mabb_bands(close, maname, len, sigma) {
        Some(v) => v,
        None => return Ok(vec![Signal::Hold; close.len()]),
    };
    let av = mabb_atr(high, low, close, atrlen);
    Ok(mabb_long_signals(
        close, high, low, &lower, &mid, &upper, &av, stopk,
    ))
}

impl Strategy for MabbLongStrategy {
    fn name(&self) -> &'static str {
        "MABB_LONG"
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
    /// 峰值≈单组(mid+sd+ATR+并行暂存)，输出与逐个调逐位一致（单测锁定）。
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
            atr_cache.insert(*a, mabb_atr(high, low, close, *a));
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
            let (mi, sd) = match mabb_core(close, m, *l) {
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
                        mabb_long_signals(close, high, low, &lower, &mi, &upper, &av, *k),
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
    fn test_mid_only_control() {
        // k=0：上破进多，中线平；绝不做空。
        let (lower, mid, upper) = bands(3);
        let sig = mabb_long_signals(
            &[101.0, 112.0, 99.0],
            &[101.0, 112.0, 99.0],
            &[101.0, 112.0, 99.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            0.0,
        );
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
        let sig2 = mabb_long_signals(
            &[101.0, 89.0, 88.0],
            &[101.0, 89.0, 88.0],
            &[101.0, 89.0, 88.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            0.0,
        );
        assert!(sig2.iter().all(|&s| s != Signal::Sell));
    }

    #[test]
    fn test_trail_fires_before_mid() {
        // entry112，extreme112，trail=112−2*5=102：low101触发，早于中线。
        let (lower, mid, upper) = bands(3);
        let sig = mabb_long_signals(
            &[101.0, 112.0, 108.0],
            &[101.0, 112.0, 108.0],
            &[101.0, 112.0, 101.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_ratchet_holds_small_dip() {
        // 新高115抬trail至105；回踩106不触发，收≥mid持有。
        let (lower, mid, upper) = bands(4);
        let sig = mabb_long_signals(
            &[101.0, 112.0, 115.0, 106.0],
            &[101.0, 112.0, 115.0, 106.0],
            &[101.0, 112.0, 115.0, 106.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 4],
            2.0,
        );
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Hold);
    }

    #[test]
    fn test_registry_all_compute_on_slice() {
        // 注册表43名必须全可在close切片上算出（否则大跑出现静默全Hold）。
        use super::super::MABB_MA_NAMES;
        let close: Vec<f64> = (0..600)
            .map(|i| 100.0 + 10.0 * ((i as f64) / 9.0).sin())
            .collect();
        let mut dead = Vec::new();
        for name in MABB_MA_NAMES {
            if mabb_bands(&close, name, 50, 2.0).is_none() {
                dead.push(name);
            }
        }
        assert!(dead.is_empty(), "ma()失败需剔除注册表：{dead:?}");
    }

    #[test]
    fn test_name_and_params() {
        let s = MabbLongStrategy {
            quints: vec![("sma".to_string(), 20, 2.5, 0.0, 0)],
        };
        assert_eq!(s.name(), "MABB_LONG");
        assert_eq!(s.param_str(0), "ma=sma,len=20,sigma=2.5,stopk=0,atrlen=0");
    }

    #[test]
    fn test_batch_matches_single() {
        // 60根合成波：batch与逐个调逐位一致。
        let n = 60;
        let close: Vec<f64> = (0..n)
            .map(|i| 100.0 + 10.0 * ((i as f64) / 5.0).sin() + i as f64 * 0.2)
            .collect();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let time: Vec<i64> = (0..n as i64).collect();
        let s = MabbLongStrategy {
            quints: vec![
                ("sma".to_string(), 10, 2.0, 0.0, 0),
                ("sma".to_string(), 10, 2.0, 2.0, 14),
                ("ema".to_string(), 12, 1.5, 3.0, 10),
            ],
        };
        let batch = s
            .generate_signals_batch(&close, &high, &low, &close, &close, &time, 0..3)
            .unwrap();
        assert_eq!(batch.len(), 3);
        for (i, b) in batch.iter().enumerate() {
            let single = s
                .generate_signals_full(&close, &high, &low, &close, &close, &time, i)
                .unwrap();
            assert_eq!(*b, single, "param {i} batch与single不一致");
        }
    }
}
