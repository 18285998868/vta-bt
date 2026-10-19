use crate::strategies::{Signal, Strategy, utc_day_index};

use super::config::VwapTrendConfig;

/// 24h滚动窗（毫秒）。连续VWAP = 过去24h成交量加权均价，与TF无关。
const ROLL_MS: i64 = 86_400_000;

/// VWAP顺势（论文口径加密适配）：线上持多、线下持空，穿越即反手（Buy/Sell，
/// 引擎按反手处理，与 SMA 单向持仓口径一致）。
/// reset=1（UTC日切）：日界处平仓且空一根bar（论文“不持仓过夜”的加密版）；
/// reset=2/3/4（亚00–08/欧08–16/美16–24UTC盘）：VWAP按8h盘块累计，
/// 只在盘内持仓（出盘即平+盘外空仓）；
/// reset=0（连续24h）：永不强制平仓。
/// VWAP NaN（预热/零成交量）→ Hold。无 OHLCV+time 的入口直接 Err（同 VWAP，P7）。
pub struct VwapTrendStrategy {
    modes: Vec<u8>,
}

impl VwapTrendStrategy {
    /// 由 [`VwapTrendConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: VwapTrendConfig) -> Self {
        Self {
            modes: config.modes(),
        }
    }
}

/// 8h盘块（亚00–08/欧08–16/美16–24UTC）。返回 slot(0/1/2)。
fn utc_slot(time_ms: i64) -> i64 {
    ((time_ms.div_euclid(3_600_000)) % 24).div_euclid(8)
}

/// session累计VWAP（典型价加权）。
/// mode=1: sess=UTC日（`vwap` 口径复用），全天 active；
/// mode=2/3/4: sess=日×3+盘块，每个盘块独立累计，仅目标盘块 active。
/// 返回 (vwap, sess_id, active)。
fn session_vwap(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    time: &[i64],
    mode: u8,
) -> (Vec<f64>, Vec<i64>, Vec<bool>) {
    let n = close.len().min(high.len()).min(low.len()).min(volume.len());
    let mut out = vec![f64::NAN; n];
    let mut sess = vec![i64::MIN; n];
    let mut active = vec![false; n];
    let mut cur = i64::MIN;
    let mut pv = 0.0;
    let mut vv = 0.0;
    for i in 0..n {
        let (s, on) = if i < time.len() {
            let day = utc_day_index(time[i]);
            if mode == 1 {
                (day, true)
            } else {
                let slot = utc_slot(time[i]);
                (day * 3 + slot, slot == mode as i64 - 2)
            }
        } else {
            (cur, mode == 1)
        };
        if s != cur {
            cur = s;
            pv = 0.0;
            vv = 0.0;
        }
        sess[i] = s;
        active[i] = on;
        let (h, l, c, v) = (high[i], low[i], close[i], volume[i]);
        if h.is_finite() && l.is_finite() && c.is_finite() && v.is_finite() && v > 0.0 {
            pv += ((h + l + c) / 3.0) * v;
            vv += v;
        }
        if vv > 0.0 {
            out[i] = pv / vv;
        }
    }
    (out, sess, active)
}

/// 过去24h滚动VWAP（前缀和 + 二分左界，O(n log n)）。
fn rolling_vwap(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    time: &[i64],
) -> Vec<f64> {
    let n = close
        .len()
        .min(high.len())
        .min(low.len())
        .min(volume.len())
        .min(time.len());
    let mut out = vec![f64::NAN; n];
    let mut pre_p = vec![0.0; n + 1];
    let mut pre_v = vec![0.0; n + 1];
    for i in 0..n {
        let (h, l, c, v) = (high[i], low[i], close[i], volume[i]);
        let (mut pv, mut vv) = (0.0, 0.0);
        if h.is_finite() && l.is_finite() && c.is_finite() && v.is_finite() && v > 0.0 {
            pv = ((h + l + c) / 3.0) * v;
            vv = v;
        }
        pre_p[i + 1] = pre_p[i] + pv;
        pre_v[i + 1] = pre_v[i] + vv;
    }
    for i in 0..n {
        let edge = time[i] - ROLL_MS;
        let l = time[..=i].partition_point(|&t| t <= edge);
        let dv = pre_v[i + 1] - pre_v[l];
        if dv > 0.0 {
            out[i] = (pre_p[i + 1] - pre_p[l]) / dv;
        }
    }
    out
}

/// 纯决策函数(可手算单测)。`bound` 为 session 边界：
/// sess 变化处平仓+空一根bar；active=false 的bar一律 Hold（盘外空仓）。
#[must_use]
pub fn vwap_trend_signals(
    close: &[f64],
    vwap: &[f64],
    bound: Option<(&[i64], &[bool])>,
) -> Vec<Signal> {
    let n = close.len().min(vwap.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if let Some((sess, active)) = bound
            && i > 0
            && i < sess.len()
            && i < active.len()
            && sess[i] != sess[i - 1]
        {
            if pos != 0 {
                sig[i] = Signal::Close;
                pos = 0;
            }
            continue; // session边界空一根bar
        }
        if let Some((_, active)) = bound
            && i < active.len()
            && !active[i]
        {
            continue; // 盘外空仓
        }
        let (c, v) = (close[i], vwap[i]);
        if !c.is_finite() || !v.is_finite() {
            continue;
        }
        // **显式三分支**, 而不是 `if c == v { continue }` + `if c > v {1} else {-1}`。
        //
        // 两者行为等价, 但后者的"相等"判定用的是**浮点相等**(`num-float-compare`):
        // 一旦将来有人在 `else` 分支上加语义(或把 `c == v` 的判断挪到别处),
        // "相等 → 走 `-1`(Sell)"的方向性偏差就会重新出现 —— 浮点相等不该承担
        // 业务语义。写成三分支后, "持平不动"是一个**显式**的分支。
        let side: i8 = if c > v {
            1
        } else if c < v {
            -1
        } else {
            continue; // 持平: 不动
        };
        if side != pos {
            sig[i] = if side == 1 { Signal::Buy } else { Signal::Sell };
            pos = side;
        }
    }
    sig
}

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    time: &[i64],
    mode: u8,
) -> Vec<Signal> {
    if mode == 1 || (2..=4).contains(&mode) {
        let (vwap, sess, active) = session_vwap(high, low, close, volume, time, mode);
        vwap_trend_signals(close, &vwap, Some((&sess, &active)))
    } else {
        let vwap = rolling_vwap(high, low, close, volume, time);
        vwap_trend_signals(close, &vwap, None)
    }
}

impl Strategy for VwapTrendStrategy {
    fn name(&self) -> &'static str {
        "VWAP_TREND"
    }

    fn param_count(&self) -> usize {
        self.modes.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("reset={}", self.modes[idx])
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        // 无 high/low/time 入口直接由 generate_signals 报错，不静默降级(P7)。
        // full 入口缺 volume/time 时 compute 内按 NaN→Hold。
        Ok(compute(
            high,
            low,
            close,
            volume,
            time,
            self.modes[param_idx],
        ))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn days(n: usize, d0: i64, per: usize) -> Vec<i64> {
        (0..n).map(|i| d0 + (i / per) as i64).collect()
    }

    #[test]
    fn test_above_buys_below_sells_reversal() {
        // 线上 Buy，线下反手 Sell。
        let close = vec![101.0, 102.0, 99.0, 98.0];
        let vwap = vec![100.0; 4];
        let sig = vwap_trend_signals(&close, &vwap, None);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Sell);
        assert_eq!(sig[3], Signal::Hold);
    }

    #[test]
    fn test_equal_holds_no_flipflop() {
        // close==vwap 不动作（防逐bar翻转）。
        let close = vec![100.0, 100.0];
        let vwap = vec![100.0, 100.0];
        let sig = vwap_trend_signals(&close, &vwap, None);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_session_boundary_closes_and_skips_one_bar() {
        // 日界：平仓 + 空一根bar，下一根按线上重新进。
        let close = vec![101.0, 102.0, 103.0, 104.0];
        let vwap = vec![100.0; 4];
        let ds = days(4, 7, 2); // [7,7,8,8]
        let on = vec![true; 4];
        let sig = vwap_trend_signals(&close, &vwap, Some((&ds, &on)));
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Close);
        assert_eq!(sig[3], Signal::Buy);
    }

    #[test]
    fn test_inactive_session_stays_flat() {
        // 盘外bar一律 Hold；已持仓在出盘首根已平（边界逻辑覆盖）。
        let close = vec![101.0, 102.0, 103.0];
        let vwap = vec![100.0; 3];
        let ds = vec![5, 5, 5];
        let on = vec![true, false, false];
        let sig = vwap_trend_signals(&close, &vwap, Some((&ds, &on)));
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Hold);
    }

    #[test]
    fn test_utc_slot_splits_three_sessions() {
        // 07:59UTC→slot0(亚)，08:00→slot1(欧)，16:00→slot2(美)。
        assert_eq!(utc_slot(7 * 3_600_000 + 59 * 60_000), 0);
        assert_eq!(utc_slot(8 * 3_600_000), 1);
        assert_eq!(utc_slot(16 * 3_600_000), 2);
        assert_eq!(utc_slot(23 * 3_600_000), 2);
    }

    #[test]
    fn test_rolling_vwap_matches_hand_calc() {
        // 3根bar同v=10：vwap恒为均值；24h窗全覆盖。
        let t: Vec<i64> = vec![0, 60_000, 120_000];
        let (h, l, c) = (
            vec![11.0, 12.0, 13.0],
            vec![9.0, 10.0, 11.0],
            vec![10.0, 11.0, 12.0],
        );
        let v = vec![10.0; 3];
        let w = rolling_vwap(&h, &l, &c, &v, &t);
        assert!((w[0] - 10.0).abs() < 1e-9);
        assert!((w[2] - 11.0).abs() < 1e-9);
    }

    #[test]
    fn test_zero_volume_holds() {
        let close = vec![101.0; 5];
        let t: Vec<i64> = (0..5).map(|i| i * 60_000).collect();
        let sig = compute(&close, &close, &close, &[0.0; 5], &t, 0);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = VwapTrendStrategy { modes: vec![1] };
        assert_eq!(s.name(), "VWAP_TREND");
        assert_eq!(s.param_str(0), "reset=1");
    }
}
