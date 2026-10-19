use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};

/// **VIDYA** 策略实现。
pub struct VidyaStrategy {
    periods: Vec<usize>,
}

impl VidyaStrategy {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

fn vidya_ma(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if n < period + 2 {
        return out;
    }
    let alpha_base = 2.0 / (period as f64 + 1.0);
    let mut v = close[0];
    let mut gains = 0.0;
    let mut losses = 0.0;
    for i in 1..=period {
        let diff = close[i] - close[i - 1];
        if diff > 0.0 {
            gains += diff;
        } else {
            losses -= diff;
        }
        v = alpha_base * close[i] + (1.0 - alpha_base) * v;
    }
    out[period] = v;
    let mut cmo_gains = gains;
    let mut cmo_losses = losses;
    for i in (period + 1)..n {
        let diff = close[i] - close[i - 1];
        let prev_diff = close[i - period] - close[i - period - 1];
        let new_gain = diff.max(0.0);
        let new_loss = (-diff).max(0.0);
        let old_gain = prev_diff.max(0.0);
        let old_loss = (-prev_diff).max(0.0);
        cmo_gains += new_gain - old_gain;
        cmo_losses += new_loss - old_loss;
        let denom = cmo_gains + cmo_losses;
        let cmo = if denom > 1e-12 {
            (cmo_gains - cmo_losses) / denom
        } else {
            0.0
        };
        let alpha = alpha_base * cmo.abs();
        v = alpha * close[i] + (1.0 - alpha) * v;
        out[i] = v;
    }
    out
}

impl Strategy for VidyaStrategy {
    fn name(&self) -> &'static str {
        "VIDYA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("vidya_{}", self.periods[idx])
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
        let period = self.periods[param_idx];
        let ma = vidya_ma(close, period);
        Ok(price_cross_signal(&ma, close))
    }
}

#[cfg(test)]
mod tests {
    use super::{VidyaStrategy, vidya_ma};
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    /// 全量重算基准: 每个点都从窗口 `(i-period+1)..=i` 重新累加 gain/loss 计算 CMO,
    /// 用于验证滚动差分统计与全量重算一致。
    fn vidya_ma_reference(close: &[f64], period: usize) -> Vec<f64> {
        let n = close.len();
        let mut out = vec![f64::NAN; n];
        if n < period + 2 {
            return out;
        }
        let alpha_base = 2.0 / (period as f64 + 1.0);
        let mut v = close[0];
        for &c in &close[1..=period] {
            v = alpha_base * c + (1.0 - alpha_base) * v;
        }
        out[period] = v;
        for i in (period + 1)..n {
            let mut g = 0.0;
            let mut l = 0.0;
            for d in (i - period + 1)..=i {
                let diff = close[d] - close[d - 1];
                if diff > 0.0 {
                    g += diff;
                } else {
                    l -= diff;
                }
            }
            let denom = g + l;
            let cmo = if denom > 1e-12 { (g - l) / denom } else { 0.0 };
            let alpha = alpha_base * cmo.abs();
            v = alpha * close[i] + (1.0 - alpha) * v;
            out[i] = v;
        }
        out
    }

    #[test]
    fn test_rollings_stats_match_full_recompute() {
        let close: Vec<f64> = (0..300)
            .map(|i| 100.0 + (i as f64 * 0.7).sin() * 30.0 + (i as f64 * 0.13).cos() * 8.0)
            .collect();
        for period in [5usize, 20, 50] {
            let rolling = vidya_ma(&close, period);
            let reference = vidya_ma_reference(&close, period);
            assert_eq!(rolling.len(), reference.len());
            for (i, (a, b)) in rolling.iter().zip(reference.iter()).enumerate() {
                if a.is_nan() || b.is_nan() {
                    assert!(
                        a.is_nan() && b.is_nan(),
                        "period={period} idx={i}: NaN mismatch"
                    );
                    continue;
                }
                let scale = b.abs().max(1.0);
                assert!(
                    (a - b).abs() / scale < 1e-9,
                    "period={period} idx={i}: rolling={a} ref={b}"
                );
            }
        }
    }

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = VidyaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
