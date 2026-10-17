use crate::strategies::{DayBar, Signal, Strategy, aggregate_days};

use super::config::RBreakerConfig;

/// R-Breaker: 前一日 H/L/C 算六价, 空仓突破进、持仓极端反转、止损、UTC 日界平仓。
/// 六价(myquant 经典式, P=(H+C+L)/3):
/// bBreak=H+2(P−L), sSetup=P+(H−L), sEnter=2P−L,
/// bEnter=2P−H, bSetup=P−(H−L), sBreak=L−2(H−P)。
/// 止损按开仓价百分比。NaN→Hold。
pub struct RBreakerStrategy {
    stops: Vec<f64>,
}

impl RBreakerStrategy {
    pub fn new(config: RBreakerConfig) -> Self {
        Self {
            stops: config.values(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Levels {
    pub b_break: f64,
    pub s_setup: f64,
    pub s_enter: f64,
    pub b_enter: f64,
    pub b_setup: f64,
    pub s_break: f64,
}

pub fn six_levels(h: f64, l: f64, c: f64) -> Option<Levels> {
    if ![h, l, c].iter().all(|v| v.is_finite()) || h < l {
        return None;
    }
    let p = (h + c + l) / 3.0;
    Some(Levels {
        b_break: h + 2.0 * (p - l),
        s_setup: p + (h - l),
        s_enter: 2.0 * p - l,
        b_enter: 2.0 * p - h,
        b_setup: p - (h - l),
        s_break: l - 2.0 * (h - p),
    })
}

/// 纯决策函数(可手算单测)。
pub fn rbreaker_signals(days: &[DayBar], close: &[f64], stop_pct: f64) -> Vec<Signal> {
    let mut sig = vec![Signal::Hold; close.len()];
    let mut pos: i8 = 0;
    let mut entry = 0.0;
    let mut hi = 0.0;
    let mut lo = 0.0;
    for (di, day) in days.iter().enumerate() {
        if di == 0 {
            continue;
        }
        let prev = &days[di - 1];
        let lv = match six_levels(prev.high, prev.low, prev.close) {
            Some(v) => v,
            None => continue,
        };
        let stop = stop_pct / 100.0;
        for i in day.start_idx..=day.end_idx.min(close.len().saturating_sub(1)) {
            let c = close[i];
            if !c.is_finite() {
                continue;
            }
            let is_eod = i == day.end_idx;
            if pos == 0 {
                if is_eod {
                    // 日末bar不新开：零持有时间纯付成本，且避免引擎隔夜持仓与策略flat分叉。
                    continue;
                }
                if c > lv.b_break {
                    sig[i] = Signal::Buy;
                    pos = 1;
                    entry = c;
                    hi = c;
                    lo = c;
                } else if c < lv.s_break {
                    sig[i] = Signal::Sell;
                    pos = -1;
                    entry = c;
                    hi = c;
                    lo = c;
                }
            } else {
                if c > hi {
                    hi = c;
                }
                if c < lo {
                    lo = c;
                }
                if is_eod {
                    sig[i] = Signal::Close;
                    pos = 0;
                    continue;
                }
                if pos == 1 {
                    if (entry - c) / entry >= stop {
                        sig[i] = Signal::Close;
                        pos = 0;
                    } else if hi > lv.s_setup && c < lv.s_enter {
                        sig[i] = Signal::Sell;
                        pos = -1;
                        entry = c;
                        hi = c;
                        lo = c;
                    }
                } else if (c - entry) / entry >= stop {
                    sig[i] = Signal::Close;
                    pos = 0;
                } else if lo < lv.b_setup && c > lv.b_enter {
                    sig[i] = Signal::Buy;
                    pos = 1;
                    entry = c;
                    hi = c;
                    lo = c;
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

impl Strategy for RBreakerStrategy {
    fn name(&self) -> &'static str {
        "RBREAKER"
    }

    fn param_count(&self) -> usize {
        self.stops.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("stop={}", self.stops[idx])
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
        let days = aggregate_days(time, open, high, low, close);
        Ok(rbreaker_signals(&days, close, self.stops[param_idx]))
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
    fn test_six_levels_formula() {
        // H=110,L=90,C=100 → P=100; bBreak=130, sSetup=120, sEnter=110,
        // bEnter=90, bSetup=80, sBreak=70。
        let lv = six_levels(110.0, 90.0, 100.0).unwrap();
        assert_eq!((lv.b_break, lv.s_setup, lv.s_enter), (130.0, 120.0, 110.0));
        assert_eq!((lv.b_enter, lv.b_setup, lv.s_break), (90.0, 80.0, 70.0));
        assert!(six_levels(90.0, 110.0, 100.0).is_none());
    }

    #[test]
    fn test_breakout_entry_and_eod_flat() {
        // 前日 H/L/C=110/90/100 → bBreak=130。次日: 100 Hold, 131 Buy, 132 Hold, 日末 Close。
        let days = vec![
            day(100.0, 110.0, 90.0, 100.0, 0, 0),
            day(100.0, 132.0, 99.0, 132.0, 1, 3),
        ];
        let close = vec![100.0, 100.0, 131.0, 132.0];
        let sig = rbreaker_signals(&days, &close, 50.0);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_no_entry_on_final_bar() {
        // 前日 H/L/C=110/90/100 → bBreak=130。次日仅最后一根突破：不进场（全 Hold）。
        let days = vec![
            day(100.0, 110.0, 90.0, 100.0, 0, 0),
            day(100.0, 132.0, 99.0, 132.0, 1, 3),
        ];
        let close = vec![100.0, 100.0, 100.0, 131.0];
        let sig = rbreaker_signals(&days, &close, 50.0);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = RBreakerStrategy { stops: vec![1.0] };
        assert_eq!(s.name(), "RBREAKER");
        assert_eq!(s.param_str(0), "stop=1");
    }
}
