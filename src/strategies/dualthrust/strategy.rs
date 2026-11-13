use crate::strategies::{DayBar, Signal, Strategy, aggregate_days};

use super::config::DualThrustConfig;

/// Dual Thrust: 按 UTC 自然日聚合, 前 N 日算 Range=max(HH−LC,HC−LL),
/// 当日开±k·R 为上下轨。flat 上破 Buy / 下破 Sell, 日内反向突破翻仓,
/// UTC 日界强制平仓。N 个完整历史日不足则全 Hold。NaN→Hold.
pub struct DualThrustStrategy {
    pairs: Vec<(usize, f64)>,
}

impl DualThrustStrategy {
    /// 由 `DualThrustConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: DualThrustConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测): 日线 + 当日逐 bar 收盘 + 当日开 + 起止下标。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn dualthrust_signals(days: &[DayBar], close: &[f64], n: usize, k: f64) -> Vec<Signal> {
    let mut sig = vec![Signal::Hold; close.len()];
    let mut pos: i8 = 0;
    for (di, day) in days.iter().enumerate() {
        if di < n {
            continue;
        }
        let hist = &days[di - n..di];
        if hist
            .iter()
            .any(|d| ![d.high, d.low, d.close].iter().all(|v| v.is_finite()))
        {
            continue;
        }
        let hh = hist
            .iter()
            .map(|d| d.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let lc = hist.iter().map(|d| d.close).fold(f64::INFINITY, f64::min);
        let hc = hist
            .iter()
            .map(|d| d.close)
            .fold(f64::NEG_INFINITY, f64::max);
        let ll = hist.iter().map(|d| d.low).fold(f64::INFINITY, f64::min);
        let range = (hh - lc).max(hc - ll);
        if range <= 0.0 || !day.open.is_finite() {
            continue;
        }
        let buy_line = day.open + k * range;
        let sell_line = day.open - k * range;
        for i in day.start_idx..=day.end_idx.min(close.len().saturating_sub(1)) {
            if !close[i].is_finite() {
                continue;
            }
            let is_eod = i == day.end_idx;
            if pos == 0 {
                if is_eod {
                    // 日末bar不新开：零持有时间纯付成本，且避免引擎隔夜持仓与策略flat分叉。
                    continue;
                }
                if close[i] > buy_line {
                    sig[i] = Signal::Buy;
                    pos = 1;
                } else if close[i] < sell_line {
                    sig[i] = Signal::Sell;
                    pos = -1;
                }
            } else {
                if is_eod {
                    sig[i] = Signal::Close;
                    pos = 0;
                    continue;
                }
                if pos == 1 {
                    if close[i] < sell_line {
                        sig[i] = Signal::Sell;
                        pos = -1;
                    }
                } else if close[i] > buy_line {
                    sig[i] = Signal::Buy;
                    pos = 1;
                }
            }
        }
        // 日切未平仓则在日末bar补 Close(上面 is_eod 已处理, 兜底)。
        if pos != 0 {
            let last = day.end_idx.min(close.len().saturating_sub(1));
            if day.start_idx <= last && sig[last] == Signal::Hold {
                sig[last] = Signal::Close;
            }
            pos = 0;
        }
    }
    sig
}

impl Strategy for DualThrustStrategy {
    fn name(&self) -> &'static str {
        "DUALTHRUST"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (n, k) = self.pairs[idx];
        format!("n={n},k={k}")
    }

    fn generate_signals_full(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (n, k) = self.pairs[param_idx];
        let days = aggregate_days(time, open, high, low, close);
        Ok(dualthrust_signals(&days, close, n, k))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
    fn test_breakout_gives_buy_then_eod_close() {
        // 1 天历史(H=110,LC=100,HC=108,LL=95 → R=max(10,13)=13);
        // 当天开 100, k=0.5 → 上轨 106.5。收盘 104 Hold, 107 Buy, 日末 108 Close。
        let days = vec![
            day(100.0, 110.0, 95.0, 108.0, 0, 0),
            day(100.0, 112.0, 99.0, 111.0, 1, 3),
        ];
        let close = vec![105.0, 104.0, 107.0, 108.0];
        let sig = dualthrust_signals(&days, &close, 1, 0.5);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_position_flat_at_day_boundary() {
        // 首日末已 Close, 次日需重新突破才进(若过夜则次日首根为 Hold 而非 Buy)。
        let days = vec![
            day(100.0, 110.0, 95.0, 108.0, 0, 0),
            day(100.0, 112.0, 99.0, 111.0, 1, 2),
            day(100.0, 112.0, 99.0, 111.0, 3, 4),
        ];
        let close = vec![105.0, 107.0, 108.0, 108.0, 108.0];
        let sig = dualthrust_signals(&days, &close, 1, 0.5);
        assert_eq!(sig[2], Signal::Close);
        assert_eq!(sig[3], Signal::Buy);
        assert_eq!(sig[4], Signal::Close);
    }

    #[test]
    fn test_no_entry_on_final_bar() {
        // 全天轨下、仅最后一根突破：不进场（全 Hold），引擎不过夜。
        let days = vec![
            day(100.0, 110.0, 95.0, 108.0, 0, 0),
            day(100.0, 112.0, 99.0, 111.0, 1, 3),
        ];
        let close = vec![105.0, 104.0, 104.0, 107.0];
        let sig = dualthrust_signals(&days, &close, 1, 0.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_insufficient_history_all_hold() {
        let days = vec![day(100.0, 110.0, 95.0, 108.0, 0, 1)];
        let close = vec![200.0, 1.0];
        let sig = dualthrust_signals(&days, &close, 5, 0.5);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = DualThrustStrategy {
            pairs: vec![(5, 0.2)],
        };
        assert_eq!(s.name(), "DUALTHRUST");
        assert_eq!(s.param_str(0), "n=5,k=0.2");
    }
}
