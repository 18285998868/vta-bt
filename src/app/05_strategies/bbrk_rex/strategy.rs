use vector_ta::indicators::bollinger_bands::{
    bollinger_bands, BollingerBandsInput, BollingerBandsParams,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkRexConfig;

/// BBRK+振幅扩张过滤：flat + close突破 **且** 方向一致 **且**
/// `(high−low) > k × mean(high−low, n)` → Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。
/// 振幅均值用前 n 根（含当根）滑动平均；NaN/窗口不足/均值非正 → 不开仓（P7）。
pub struct BbrkRexStrategy {
    pairs: Vec<(f64, usize, usize, f64)>,
}

impl BbrkRexStrategy {
    pub fn new(config: BbrkRexConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；8参为OHLC+三轨+窗长的最小完备集)。
#[allow(clippy::too_many_arguments)]
pub fn bbrk_rex_signals(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    win_n: usize,
    mult: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(high.len())
        .min(low.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    // 当根振幅与前20根均值（前缀和 O(n)）。
    let mut range = vec![f64::NAN; n];
    for i in 0..n {
        if high[i].is_finite() && low[i].is_finite() && high[i] >= low[i] {
            range[i] = high[i] - low[i];
        }
    }
    let mut pre = vec![0.0; n + 1];
    let mut cnt = vec![0usize; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i];
        cnt[i + 1] = cnt[i];
        if range[i].is_finite() {
            pre[i + 1] += range[i];
            cnt[i + 1] += 1;
        }
    }
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
            // 扩张门：整窗 n 根有效振幅 + 当根振幅 > k×均值。
            if win_n == 0 {
                continue;
            }
            let lo = (i + 1).saturating_sub(win_n);
            if i + 1 < win_n || cnt[i + 1] - cnt[lo] != win_n {
                continue;
            }
            let mean = (pre[i + 1] - pre[lo]) / win_n as f64;
            if !mean.is_finite() || mean <= 0.0 || !range[i].is_finite() {
                continue;
            }
            if range[i] <= mult * mean {
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
    win_n: usize,
    mult: f64,
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
    Ok(bbrk_rex_signals(
        open,
        high,
        low,
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        win_n,
        mult,
    ))
}

impl Strategy for BbrkRexStrategy {
    fn name(&self) -> &'static str {
        "BBRK_REX"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, l, n, m) = self.pairs[idx];
        format!("sigma={s},bblen={l},rexn={n},rexmult={m}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, l, n, m) = self.pairs[param_idx];
        compute(close, close, close, close, s, l, n, m)
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
        let (s, l, n, m) = self.pairs[param_idx];
        compute(open, high, low, close, s, l, n, m)
    }

    /// 批量版：BB 按去重后的 (sigma, bblen) 只算一次，再并行跑各 (N, k)。
    /// 输出与逐个调逐位一致（单测锁定）。
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
            bollinger_bands, BollingerBandsInput, BollingerBandsParams,
        };
        let mut bbmap: HashMap<(u64, usize), crate::strategies::CachedBands> =
            HashMap::new();
        for idx in range.clone() {
            let (s, l, _, _) = self.pairs[idx];
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
                let (s, l, n, m) = self.pairs[idx];
                match bbmap.get(&(s.to_bits(), l)) {
                    Some(Some((lower, mid, upper))) => Ok(bbrk_rex_signals(
                        open, high, low, close, lower, mid, upper, n, m,
                    )),
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

    fn flat_bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_expanded_breakout_buys() {
        // 前20根振幅=2，当根振幅=14 > 1.5×均值(≈2.6) → Buy。
        let n = 21;
        let (lower, mid, upper) = flat_bands(n);
        let open = vec![100.0; n];
        let mut high = vec![101.0; n];
        let mut low = vec![99.0; n];
        let mut close = vec![100.0; n];
        high[n - 1] = 113.0;
        low[n - 1] = 99.0;
        close[n - 1] = 112.0;
        let sig = bbrk_rex_signals(&open, &high, &low, &close, &lower, &mid, &upper, 20, 1.5);
        assert_eq!(sig[n - 1], Signal::Buy);
    }

    #[test]
    fn test_normal_range_breakout_blocked() {
        // 当根振幅=2 == 均值2，不 > 1.5×2 → Hold。
        let n = 21;
        let (lower, mid, upper) = flat_bands(n);
        let open = vec![100.0; n];
        let high = vec![101.0; n];
        let low = vec![99.0; n];
        let mut close = vec![100.0; n];
        close[n - 1] = 111.0; // 突破但振幅正常
        let sig = bbrk_rex_signals(&open, &high, &low, &close, &lower, &mid, &upper, 20, 1.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_short_window_holds() {
        // 不足 win_n 根 → 不开仓。
        let (lower, mid, upper) = flat_bands(5);
        let sig = bbrk_rex_signals(
            &vec![100.0; 5],
            &vec![113.0; 5],
            &vec![99.0; 5],
            &vec![112.0; 5],
            &lower,
            &mid,
            &upper,
            20,
            2.0,
        );
        assert!(sig.iter().all(|&s| s == Signal::Hold));
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
        let st = BbrkRexStrategy {
            pairs: vec![(2.5, 20, 7, 1.5), (2.5, 20, 7, 2.5), (2.0, 10, 5, 2.0)],
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
        let s = BbrkRexStrategy {
            pairs: vec![(2.5, 20, 7, 2.0)],
        };
        assert_eq!(s.name(), "BBRK_REX");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,rexn=7,rexmult=2");
    }
}
