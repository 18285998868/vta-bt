use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};

use crate::strategies::{Signal, Strategy, utc_day_index};

use super::config::VwapConfig;

/// VWAP拉伸回归。session VWAP按UTC日界重置(典型价加权), ATR14归一偏离度。
/// Buy: 空头拉伸 `(vwap-close)/atr > th` 后首次收回(严格); Sell 镜像。
/// volume<=0/非有限、ATR非正非有限一律 Hold。无 time/high/low 的入口直接 Err,
/// 不做静默降级(日界与典型价无从算起)。
pub struct VwapStrategy {
    threshs: Vec<f64>,
}

impl VwapStrategy {
    /// 由 [`VwapConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: VwapConfig) -> Self {
        Self {
            threshs: config.values(),
        }
    }
}

#[must_use]
fn session_vwap(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    time: &[i64],
) -> Vec<f64> {
    // 四条数组都按 `n` 索引, 故 `n` 必须是**四者的最小长度** ——
    // 只取 `close.len()` 时, 任一条更短就会越界 panic。
    // (生产路径由 `app::validate_columns` 保证六列等长, 但本函数是
    // crate 内的 `pub` 面, 调用方不该替它承担这个前提。)
    // 兄弟策略 `vwap_trend::session_vwap` 用的是同一口径。
    let n = close.len().min(high.len()).min(low.len()).min(volume.len());
    let mut out = vec![f64::NAN; n];
    let mut day = i64::MIN;
    let mut pv = 0.0;
    let mut vv = 0.0;
    for i in 0..n {
        let d = if i < time.len() {
            utc_day_index(time[i])
        } else {
            day
        };
        if d != day {
            day = d;
            pv = 0.0;
            vv = 0.0;
        }
        let (h, l, c, v) = (high[i], low[i], close[i], volume[i]);
        if h.is_finite() && l.is_finite() && c.is_finite() && v.is_finite() && v > 0.0 {
            pv += ((h + l + c) / 3.0) * v;
            vv += v;
        }
        if vv > 0.0 {
            out[i] = pv / vv;
        }
    }
    out
}

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    time: &[i64],
    th: f64,
) -> Vec<Signal> {
    let n = close.len();
    let mut sig = vec![Signal::Hold; n];
    let vwap = session_vwap(high, low, close, volume, time);
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(14) },
    )) {
        Ok(v) => v.values,
        Err(_) => return sig,
    };
    for i in 1..n.min(atr_vals.len()) {
        let (v0, v1, a0, a1) = (vwap[i - 1], vwap[i], atr_vals[i - 1], atr_vals[i]);
        if !(v0.is_finite() && v1.is_finite() && a0 > 0.0 && a1 > 0.0) {
            continue;
        }
        let dn_prev = (v0 - close[i - 1]) / a0;
        let dn_cur = (v1 - close[i]) / a1;
        let up_prev = -dn_prev;
        let up_cur = -dn_cur;
        if dn_prev > th && dn_cur < th {
            sig[i] = Signal::Buy;
        } else if up_prev > th && up_cur < th {
            sig[i] = Signal::Sell;
        }
    }
    sig
}

impl Strategy for VwapStrategy {
    fn name(&self) -> &'static str {
        "VWAP"
    }

    fn param_count(&self) -> usize {
        self.threshs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("thresh={}", self.threshs[idx])
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
        Ok(compute(
            high,
            low,
            close,
            volume,
            time,
            self.threshs[param_idx],
        ))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bars(n: usize, t0: i64, step: i64) -> Vec<i64> {
        (0..n).map(|i| t0 + i as i64 * step).collect()
    }

    #[test]
    fn test_dip_recovery_buys() {
        // 平价基线后急跌再收回: 空头拉伸超阈后首次收回给 Buy。
        let n = 60;
        let t = bars(n, 1_700_000_000_000, 60_000);
        let mut close = vec![100.0; n];
        close[30..35].fill(90.0);
        let high = close.clone();
        let low = close.clone();
        let volume = vec![10.0; n];
        let sig = compute(&high, &low, &close, &volume, &t, 1.5);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_spike_recovery_sells() {
        let n = 60;
        let t = bars(n, 1_700_000_000_000, 60_000);
        let mut close = vec![100.0; n];
        close[30..35].fill(110.0);
        let high = close.clone();
        let low = close.clone();
        let volume = vec![10.0; n];
        let sig = compute(&high, &low, &close, &volume, &t, 1.5);
        assert!(sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_zero_volume_holds() {
        let n = 30;
        let t = bars(n, 1_700_000_000_000, 60_000);
        let close = vec![100.0; n];
        let sig = compute(&close, &close, &close, &vec![0.0; n], &t, 1.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = VwapStrategy { threshs: vec![2.0] };
        assert_eq!(s.name(), "VWAP");
        assert_eq!(s.param_str(0), "thresh=2");
    }
}
