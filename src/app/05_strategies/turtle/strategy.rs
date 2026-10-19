use crate::strategies::{Signal, Strategy, rolling_highest, rolling_lowest, shift_right_1};

use super::config::TurtleConfig;

/// 海龟 Donchian: 入场通道突破进(flat), 出场通道反穿出(持有)。
/// 状态机保证引擎重放一致: flat→Buy/Sell, long→Close(破出场下沿), short→Close(破出场上沿)。
/// NaN→Hold。ATR 仓位管理 vta 固定名义表达不了, 只复现信号层(差异见研究报告)。
pub struct TurtleStrategy {
    pairs: Vec<(usize, usize)>,
}

impl TurtleStrategy {
    /// 由 [`TurtleConfig`] 构造; 参数网格由该配置展开。
    pub fn new(config: TurtleConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测): 入场高低轨 + 出场高低轨。
pub fn turtle_signals(
    close: &[f64],
    entry_hi: &[f64],
    entry_lo: &[f64],
    exit_hi: &[f64],
    exit_lo: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(entry_hi.len())
        .min(entry_lo.len())
        .min(exit_hi.len())
        .min(exit_lo.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 1..n {
        let vs = [
            close[i - 1],
            close[i],
            entry_hi[i - 1],
            entry_hi[i],
            entry_lo[i - 1],
            entry_lo[i],
            exit_hi[i],
            exit_lo[i],
        ];
        if vs.iter().any(|v| !v.is_finite()) {
            continue;
        }
        if pos == 0 {
            if close[i - 1] <= entry_hi[i - 1] && close[i] > entry_hi[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i - 1] >= entry_lo[i - 1] && close[i] < entry_lo[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if close[i - 1] >= exit_lo[i - 1] && close[i] < exit_lo[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if close[i - 1] <= exit_hi[i - 1] && close[i] > exit_hi[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

fn signals_from_ohlc(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    entry: usize,
    exit: usize,
) -> Vec<Signal> {
    // 通道右移: 与前 N 根极值比较(经典口径, 含当根则永不穿越)。
    let hh_e = shift_right_1(&rolling_highest(high, entry));
    let ll_e = shift_right_1(&rolling_lowest(low, entry));
    let hh_x = shift_right_1(&rolling_highest(high, exit));
    let ll_x = shift_right_1(&rolling_lowest(low, exit));
    turtle_signals(close, &hh_e, &ll_e, &hh_x, &ll_x)
}

impl Strategy for TurtleStrategy {
    fn name(&self) -> &'static str {
        "TURTLE"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (e, x) = self.pairs[idx];
        format!("entry={e},exit={x}")
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (e, x) = self.pairs[param_idx];
        Ok(signals_from_ohlc(high, low, close, e, x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};
    use crate::strategies::Strategy;

    #[test]
    fn test_entry_buy_then_exit_close() {
        // 入场轨恒 100/90, 出场轨恒 95/85: 涨破 100 进多, 跌破 85 离场。
        let close = vec![95.0, 98.0, 101.0, 102.0, 90.0, 84.0, 80.0];
        let he = vec![100.0; 7];
        let le = vec![90.0; 7];
        let hx = vec![95.0; 7];
        let lx = vec![85.0; 7];
        let sig = turtle_signals(&close, &he, &le, &hx, &lx);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[5], Signal::Close);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Buy).count(), 1);
    }

    #[test]
    fn test_nan_holds() {
        let close = vec![101.0, 102.0];
        let nan = vec![f64::NAN, f64::NAN];
        let sig = turtle_signals(&close, &nan, &nan, &nan, &nan);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_flat_then_rise_enters_once() {
        // 横盘 10 根(通道在 100 建成)后逐根上涨: 首根涨破给 Buy, 之后持有。
        let mut close = vec![100.0; 10];
        close.extend((1..=50).map(|i| 100.0 + i as f64));
        let sig = signals_from_ohlc(&close, &close, &close, 5, 3);
        assert_eq!(sig.iter().filter(|s| **s == Signal::Buy).count(), 1);
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_strategy_param_count() {
        let c = TurtleConfig {
            entry: PeriodParam::Range(RangeParam {
                start: 20,
                end: 55,
                step: 35,
            }),
            exit: PeriodParam::Range(RangeParam {
                start: 10,
                end: 20,
                step: 10,
            }),
        };
        let s = TurtleStrategy::new(c);
        assert_eq!(s.param_count(), 4);
        assert_eq!(s.param_str(0), "entry=20,exit=10");
    }
}
