use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkOrthConfig;

/// 形态回看窗（预注册固定 10 根，不调参，防 P2）。
pub const ORTH_W: usize = 10;

/// 正统布林形态对照（Bollinger 2001：W 底做多 / M 顶做空 + %b 背离）：
/// BB(20, sma, σ) 经典口径，%b=(close-lower)/(upper-lower)。
/// W 有效 ⇔ 回看窗内最近两根 %b<0 的 tag 呈上升（第二根 %b 更高，
/// 价格可创新低——背离即成立）；确认根 `close>mid`（吃上半段）才进多头。
/// M 有效 ⇔ 最近两根 %b>1 的 tag 呈下降；确认根 `close<mid` 进空头。
/// 离场与 BBRK 同形（多头 `close<=mid` / 空头 `close>=mid` 平仓），
/// 以隔离“进场定义”差异。NaN → Hold（P7）。
/// 注：纯价格版正统（未含 MFI 量能确认——引擎 `generate_signals`
/// 为 close 口径，MFI 需 HLCV 全量，列为后续工作）。
pub struct BbrkOrthStrategy {
    sigmas: Vec<f64>,
}

impl BbrkOrthStrategy {
    pub fn new(config: BbrkOrthConfig) -> Self {
        Self {
            sigmas: config.values(),
        }
    }
}

fn pct_b(close: f64, lower: f64, upper: f64) -> f64 {
    if !close.is_finite() || !lower.is_finite() || !upper.is_finite() {
        return f64::NAN;
    }
    let w = upper - lower;
    if !w.is_finite() || w <= 0.0 {
        return f64::NAN;
    }
    (close - lower) / w
}

/// 窗 `[lo, i)` 内最近两根满足 `tag` 的下标（ precipitate 由调用方给条件）。
/// 返回 (a, b)，a<b<i；不足两根 → None。
fn last_two_tags(
    pct: &[f64],
    lo: usize,
    i: usize,
    tag: impl Fn(f64) -> bool,
) -> Option<(usize, usize)> {
    let mut found: Option<(usize, usize)> = None;
    let mut prev: Option<usize> = None;
    for (j, &v) in pct.iter().enumerate().take(i).skip(lo) {
        if v.is_finite() && tag(v) {
            if let Some(a) = prev {
                found = Some((a, j));
            }
            prev = Some(j);
        }
    }
    found
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_orth_signals(close: &[f64], lower: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let pct: Vec<f64> = (0..n)
        .map(|k| pct_b(close[k], lower[k], upper[k]))
        .collect();
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], mid[i], upper[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            let lo = i.saturating_sub(ORTH_W);
            // W 底：最近两根 %b<0 tag 上升，且确认根站上中线。
            let w_ok = last_two_tags(&pct, lo, i, |v| v < 0.0)
                .is_some_and(|(a, b)| pct[b] > pct[a])
                && close[i] > mid[i];
            // M 顶：最近两根 %b>1 tag 下降，且确认根跌破中线。
            let m_ok = last_two_tags(&pct, lo, i, |v| v > 1.0)
                .is_some_and(|(a, b)| pct[b] < pct[a])
                && close[i] < mid[i];
            if w_ok {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if m_ok {
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
        } else if close[i] >= mid[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

impl Strategy for BbrkOrthStrategy {
    fn name(&self) -> &'static str {
        "BBRK_ORTH"
    }

    fn param_count(&self) -> usize {
        self.sigmas.len()
    }

    fn param_str(&self, idx: usize) -> String {
        // W 预注册固定，不进 param_str（parquet 零改动口径）。
        format!("sigma={}", self.sigmas[idx])
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        _high: &[f64],
        _low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        self.compute(close, self.sigmas[param_idx])
    }
}

impl BbrkOrthStrategy {
    fn compute(&self, close: &[f64], sigma: f64) -> crate::error::Result<Vec<Signal>> {
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
        Ok(bbrk_orth_signals(
            close,
            &bb.lower_band,
            &bb.middle_band,
            &bb.upper_band,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    // 常数带 lower=90/mid=100/upper=110（宽20）：close<90 ⇔ %b<0，close>110 ⇔ %b>1。
    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_w_bottom_divergence_buys() {
        // %b: -0.10, 0.25, -0.05(上升), 0.30, 确认根101>mid → Buy@4。
        let (lower, mid, upper) = bands(5);
        let sig = bbrk_orth_signals(&[88.0, 95.0, 89.0, 96.0, 101.0], &lower, &mid, &upper);
        assert_eq!(sig[4], Signal::Buy);
        assert!(sig[..4].iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_w_without_divergence_blocked() {
        // 第二 tag %b 更低（-0.05 → -0.10，无背离）→ 全 Hold。
        let (lower, mid, upper) = bands(5);
        let sig = bbrk_orth_signals(&[89.0, 95.0, 88.0, 96.0, 101.0], &lower, &mid, &upper);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_m_top_mirror_sells() {
        // %b: 1.10, 0.75, 1.05(下降), 0.70, 确认根99<mid → Sell@4。
        let (lower, mid, upper) = bands(5);
        let sig = bbrk_orth_signals(&[112.0, 105.0, 111.0, 104.0, 99.0], &lower, &mid, &upper);
        assert_eq!(sig[4], Signal::Sell);
    }

    #[test]
    fn test_stale_setup_expires() {
        // tag 只在 0/1，两确认根在 11/12（超窗 W=10）→ 全 Hold。
        let n = 13;
        let (lower, mid, upper) = bands(n);
        let mut close = vec![95.0; n];
        close[0] = 88.0;
        close[1] = 89.0;
        close[11] = 101.0;
        close[12] = 101.0;
        let sig = bbrk_orth_signals(&close, &lower, &mid, &upper);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_exit_same_as_bbrk() {
        let (lower, mid, upper) = bands(6);
        let sig = bbrk_orth_signals(
            &[88.0, 95.0, 89.0, 96.0, 101.0, 100.0],
            &lower,
            &mid,
            &upper,
        );
        assert_eq!(sig[4], Signal::Buy);
        assert_eq!(sig[5], Signal::Close);
    }

    #[test]
    fn test_nan_hold() {
        let nan = vec![f64::NAN, f64::NAN];
        let sig = bbrk_orth_signals(&nan, &nan, &nan, &nan);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkOrthStrategy { sigmas: vec![2.5] };
        assert_eq!(s.name(), "BBRK_ORTH");
        assert_eq!(s.param_str(0), "sigma=2.5");
        assert_eq!(ORTH_W, 10);
    }
}
