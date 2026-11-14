use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkVolEmaConfig;

/// BBRK+量能门：flat + close突破 **且** 方向一致 **且**
/// `volume[i] > mult × mean(volume[i−N+1..=i])` → Buy/Sell；
///
/// **均值窗含当根**(`i−N+1 ..= i`, 共 N 根): 前缀和 `pre[i+1] − pre[i+1−N]`
/// 覆盖的正是这 N 根。含当根 ⇒ 阈值 `mult × mean` 被当根自身抬高 ⇒ 量能突增
/// 判定**更难**通过(更保守)。兄弟策略 `bbrk_rex` 用的是同一约定
/// ("振幅均值用前 n 根(含当根)滑动平均")。
/// 持仓后中线离场（与 BBRK 相同）。
/// 均值窗要求整窗有效且>0（volume<=0/NaN 即 portion 缺失 → 不开仓，P7）。
pub struct BbrkVolEmaStrategy {
    pairs: Vec<(f64, usize, usize, f64)>,
}

impl BbrkVolEmaStrategy {
    /// 由 `BbrkVolEmaConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkVolEmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn bbrk_vol_ema_signals(
    open: &[f64],
    close: &[f64],
    volume: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    win_n: usize,
    mult: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(volume.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    // 成交量前缀和 O(n)。
    let mut pre = vec![0.0; n + 1];
    let mut cnt = vec![0usize; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i];
        cnt[i + 1] = cnt[i];
        if volume[i].is_finite() && volume[i] > 0.0 {
            pre[i + 1] += volume[i];
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
            // 量能门：整窗有效 + 当根 > mult×均值。
            if win_n == 0 {
                continue;
            }
            let lo = (i + 1).saturating_sub(win_n);
            if i + 1 < win_n || cnt[i + 1] - cnt[lo] != win_n {
                continue;
            }
            let mean = (pre[i + 1] - pre[lo]) / win_n as f64;
            if !mean.is_finite() || mean <= 0.0 || !volume[i].is_finite() {
                continue;
            }
            if volume[i] <= mult * mean {
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
    close: &[f64],
    volume: &[f64],
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
            matype: Some("ema".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    Ok(bbrk_vol_ema_signals(
        open,
        close,
        volume,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        win_n,
        mult,
    ))
}

impl Strategy for BbrkVolEmaStrategy {
    fn name(&self) -> &'static str {
        "BBRK_VOL_EMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, n, m) = self.pairs[idx];
        format!("sigma={s},bblen={b},voln={n},volmult={m}")
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, b, n, m) = self.pairs[param_idx];
        compute(bars.open(), bars.close(), bars.volume(), s, b, n, m)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_volume_surge_breakout_buys() {
        // 前5根量=10，当根量=100 > 1.5×均值(25) → Buy。
        let (lower, mid, upper) = bands(6);
        let open = vec![100.0; 6];
        let mut close = vec![100.0; 6];
        close[5] = 112.0;
        let mut vol = vec![10.0; 6];
        vol[5] = 100.0;
        let sig = bbrk_vol_ema_signals(&open, &close, &vol, &lower, &mid, &upper, 5, 1.5);
        assert_eq!(sig[5], Signal::Buy);
    }

    #[test]
    fn test_flat_volume_breakout_blocked() {
        // 量全10：均值10，当根10 ≯ 15 → Hold。
        let (lower, mid, upper) = bands(6);
        let open = vec![100.0; 6];
        let mut close = vec![100.0; 6];
        close[5] = 112.0;
        let vol = vec![10.0; 6];
        let sig = bbrk_vol_ema_signals(&open, &close, &vol, &lower, &mid, &upper, 5, 1.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_zero_volume_window_holds() {
        // 窗内有零成交量 → 整窗无效 → Hold。
        let (lower, mid, upper) = bands(6);
        let open = vec![100.0; 6];
        let mut close = vec![100.0; 6];
        close[5] = 112.0;
        let mut vol = vec![10.0; 6];
        vol[2] = 0.0;
        vol[5] = 100.0;
        let sig = bbrk_vol_ema_signals(&open, &close, &vol, &lower, &mid, &upper, 5, 1.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkVolEmaStrategy {
            pairs: vec![(2.5, 20, 10, 1.5)],
        };
        assert_eq!(s.name(), "BBRK_VOL_EMA");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,voln=10,volmult=1.5");
    }
}
