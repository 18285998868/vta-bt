use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::{BbrkRexConfig, REX_MULT};

/// BBRK+振幅扩张过滤：flat + close突破 **且** 方向一致 **且**
/// `(high−low) > 1.5 × mean(high−low, n)` → Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。
/// 振幅均值用前 n 根（含当根）滑动平均；NaN/窗口不足/均值非正 → 不开仓（P7）。
pub struct BbrkRexStrategy {
    pairs: Vec<(f64, usize)>,
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
            // 扩张门：整窗 n 根有效振幅 + 当根振幅 > 1.5×均值。
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
            if range[i] <= REX_MULT * mean {
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
    win_n: usize,
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
    Ok(bbrk_rex_signals(
        open,
        high,
        low,
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        win_n,
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
        let (s, n) = self.pairs[idx];
        format!("sigma={s},rexn={n}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, n) = self.pairs[param_idx];
        compute(close, close, close, close, s, n)
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
        let (s, n) = self.pairs[param_idx];
        compute(open, high, low, close, s, n)
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
        let sig = bbrk_rex_signals(&open, &high, &low, &close, &lower, &mid, &upper, 20);
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
        let sig = bbrk_rex_signals(&open, &high, &low, &close, &lower, &mid, &upper, 20);
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
        );
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkRexStrategy {
            pairs: vec![(2.5, 7)],
        };
        assert_eq!(s.name(), "BBRK_REX");
        assert_eq!(s.param_str(0), "sigma=2.5,rexn=7");
    }
}
