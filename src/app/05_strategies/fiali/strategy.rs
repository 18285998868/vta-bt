use crate::strategies::{DayBar, Signal, Strategy, aggregate_days};

use super::config::FialiConfig;

/// 菲阿里四价：上轨=昨日最高，下轨=昨日最低；无持仓突破开仓，
/// 持仓触开盘价止损；UTC 日界强制平仓；每方向每日限1次。
/// NaN→Hold。
pub struct FialiStrategy;

impl FialiStrategy {
    pub fn new(_config: FialiConfig) -> Self {
        Self
    }
}

/// 纯决策函数(可手算单测)。
pub fn fiali_signals(days: &[DayBar], open: &[f64], close: &[f64]) -> Vec<Signal> {
    let n = close.len().min(open.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    let mut fired_long: bool;
    let mut fired_short: bool;
    for (di, day) in days.iter().enumerate() {
        if di == 0 {
            continue;
        }
        let prev = &days[di - 1];
        if ![prev.high, prev.low].iter().all(|v| v.is_finite()) {
            continue;
        }
        fired_long = false;
        fired_short = false;
        for i in day.start_idx..=day.end_idx.min(n.saturating_sub(1)) {
            let (o, c) = (open[i], close[i]);
            if !o.is_finite() || !c.is_finite() {
                continue;
            }
            let is_eod = i == day.end_idx;
            if pos == 0 {
                if !fired_long && c > prev.high {
                    sig[i] = Signal::Buy;
                    pos = 1;
                    fired_long = true;
                } else if !fired_short && c < prev.low {
                    sig[i] = Signal::Sell;
                    pos = -1;
                    fired_short = true;
                }
            } else {
                if is_eod {
                    sig[i] = Signal::Close;
                    pos = 0;
                    continue;
                }
                let stop_hit = (pos == 1 && c < o) || (pos == -1 && c > o);
                if stop_hit {
                    sig[i] = Signal::Close;
                    pos = 0;
                }
            }
        }
        if pos != 0 {
            let last = day.end_idx.min(n.saturating_sub(1));
            if day.start_idx <= last && sig[last] == Signal::Hold {
                sig[last] = Signal::Close;
            }
            pos = 0;
        }
    }
    sig
}

impl Strategy for FialiStrategy {
    fn name(&self) -> &'static str {
        "FIALI"
    }

    fn param_count(&self) -> usize {
        1
    }

    fn param_str(&self, _idx: usize) -> String {
        "base".to_string()
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        _param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        Ok(vec![Signal::Hold; close.len()])
    }

    fn generate_signals_full(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        time: &[i64],
        _param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let days = aggregate_days(time, open, high, low, close);
        Ok(fiali_signals(&days, open, close))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn day(open: f64, high: f64, low: f64, close: f64, s: usize, e: usize) -> DayBar {
        DayBar {
            day: 0,
            open,
            high,
            low,
            close,
            start_idx: s,
            end_idx: e,
        }
    }

    #[test]
    fn test_breakout_once_then_eod() {
        // 前日高低 110/90；次日开 100: 105 Hold, 111 Buy, 112 Hold(已触发过), 日末 Close。
        let days = vec![
            day(100.0, 110.0, 90.0, 105.0, 0, 0),
            day(100.0, 112.0, 99.0, 112.0, 1, 4),
        ];
        let open = vec![100.0, 100.0, 100.0, 100.0, 100.0];
        let close = vec![105.0, 105.0, 111.0, 112.0, 112.0];
        let sig = fiali_signals(&days, &open, &close);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Hold);
        assert_eq!(sig[4], Signal::Close);
    }

    #[test]
    fn test_open_stop_closes_long() {
        // 持多后收盘跌破当日开 → Close。
        let days = vec![
            day(100.0, 110.0, 90.0, 105.0, 0, 0),
            day(100.0, 115.0, 98.0, 99.0, 1, 3),
        ];
        let open = vec![100.0, 100.0, 100.0, 100.0];
        let close = vec![105.0, 111.0, 99.0, 99.0];
        let sig = fiali_signals(&days, &open, &close);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_name() {
        let s = FialiStrategy;
        assert_eq!(s.name(), "FIALI");
        assert_eq!(s.param_str(0), "base");
        let _ = Strategy::param_count(&s);
    }
}
