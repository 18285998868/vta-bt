use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::moving_averages::ma::{MaData, ma};

use crate::strategies::{Signal, Strategy};

use super::config::MaAtrConfig;

pub struct MaAtrStrategy {
    quints: Vec<(String, usize, f64, usize)>,
}

impl MaAtrStrategy {
    pub fn new(config: MaAtrConfig) -> Self {
        Self {
            quints: config.quints(),
        }
    }
}

pub fn ma_atr_ma(close: &[f64], maname: &str, len: usize) -> Option<Vec<f64>> {
    if len == 0 {
        return None;
    }
    let v = ma(maname, MaData::Slice(close), len).ok()?;
    if v.len() != close.len() {
        return None;
    }
    Some(v)
}

pub fn ma_atr_atr(high: &[f64], low: &[f64], close: &[f64], atrlen: usize) -> Vec<f64> {
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

pub fn ma_atr_signals(close: &[f64], ma_vals: &[f64], atr_vals: &[f64], k: f64) -> Vec<Signal> {
    let n = close.len().min(ma_vals.len()).min(atr_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], ma_vals[i], atr_vals[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            let upper = ma_vals[i] + k * atr_vals[i];
            let lower = ma_vals[i] - k * atr_vals[i];
            if close[i] > upper {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if ![close[i], ma_vals[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] <= ma_vals[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if ![close[i], ma_vals[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= ma_vals[i] {
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
    maname: &str,
    len: usize,
    k: f64,
    atrlen: usize,
) -> crate::error::Result<Vec<Signal>> {
    let ma_vals = match ma_atr_ma(close, maname, len) {
        Some(v) => v,
        None => return Ok(vec![Signal::Hold; close.len()]),
    };
    let atr_vals = ma_atr_atr(high, low, close, atrlen);
    Ok(ma_atr_signals(close, &ma_vals, &atr_vals, k))
}

impl Strategy for MaAtrStrategy {
    fn name(&self) -> &'static str {
        "MA_ATR"
    }
    fn param_count(&self) -> usize {
        self.quints.len()
    }
    fn param_str(&self, idx: usize) -> String {
        let (m, l, k, a) = &self.quints[idx];
        format!("ma={},len={},mult={},atrlen={}", m, l, k, a)
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
        let (m, l, k, a) = &self.quints[param_idx];
        compute(high, low, close, m, *l, *k, *a)
    }
    fn generate_signals_batch(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        range: std::ops::Range<usize>,
    ) -> crate::error::Result<Vec<Vec<Signal>>> {
        use rayon::prelude::*;
        use std::collections::{HashMap, HashSet};
        let idxs: Vec<usize> = range.collect();
        let mut alens: HashSet<usize> = HashSet::new();
        for &i in &idxs {
            alens.insert(self.quints[i].3);
        }
        let mut atr_cache: HashMap<usize, Vec<f64>> = HashMap::new();
        for a in &alens {
            atr_cache.insert(*a, ma_atr_atr(high, low, close, *a));
        }
        let mut groups: HashMap<(String, usize), Vec<usize>> = HashMap::new();
        for &i in &idxs {
            let (m, l, _, _) = &self.quints[i];
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
            let mv = match ma_atr_ma(close, m, *l) {
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
                    let (_, _, k, a) = &self.quints[i];
                    let av = atr_cache
                        .get(a)
                        .cloned()
                        .unwrap_or(vec![f64::NAN; close.len()]);
                    (i, ma_atr_signals(close, &mv, &av, *k))
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

    #[test]
    fn test_name_and_params() {
        let s = MaAtrStrategy {
            quints: vec![("sma".to_string(), 20, 2.0, 14)],
        };
        assert_eq!(s.name(), "MA_ATR");
        assert_eq!(s.param_str(0), "ma=sma,len=20,mult=2,atrlen=14");
    }

    #[test]
    fn test_batch_matches_single() {
        let n = 60;
        let close: Vec<f64> = (0..n)
            .map(|i| 100.0 + (i as f64 * 0.2).sin() * 6.0 + i as f64 * 0.1)
            .collect();
        let high: Vec<f64> = close.iter().map(|v| v + 2.0).collect();
        let low: Vec<f64> = close.iter().map(|v| v - 2.0).collect();
        let time: Vec<i64> = (0..n as i64).collect();
        let s = MaAtrStrategy {
            quints: vec![
                ("sma".to_string(), 10, 1.5, 10),
                ("sma".to_string(), 10, 2.0, 14),
            ],
        };
        let batch = s
            .generate_signals_batch(&close, &high, &low, &close, &[], &time, 0..2)
            .unwrap();
        assert_eq!(batch.len(), 2);
        for (i, b) in batch.iter().enumerate() {
            let single = s
                .generate_signals_full(&close, &high, &low, &close, &[], &time, i)
                .unwrap();
            assert_eq!(*b, single, "param {}", i);
        }
    }
}
