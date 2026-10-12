use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy, rolling_highest, rolling_lowest, shift_right_1};

use super::config::BbrkTrailConfig;

/// 追踪窗固定值（Chandelier 常用 22 周期，ATR 用 14 经典值）。
const TRAIL_N: usize = 22;

/// BBRK+ATR追踪离场：flat + close>upper → Buy；flat + close<lower → Sell；
/// 持仓后只看追踪线（多头 close < 近N根最高high(不含当根) − k·ATR 即 Close，
/// 空头镜像）。中线不再触发离场——让盈利奔跑、代价是亏损也跑更远，
/// 这正是本变体要量化的 trade-off。NaN→Hold（P7）。
pub struct BbrkTrailStrategy {
    pairs: Vec<(f64, f64)>,
}

impl BbrkTrailStrategy {
    pub fn new(config: BbrkTrailConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。`trail_long/short` 为调用方算好的逐 bar 追踪线。
pub fn bbrk_trail_signals(
    close: &[f64],
    lower: &[f64],
    upper: &[f64],
    trail_long: &[f64],
    trail_short: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(upper.len())
        .min(trail_long.len())
        .min(trail_short.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if ![close[i], trail_long[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] < trail_long[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if ![close[i], trail_short[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > trail_short[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    trailk: f64,
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
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(14) },
    )) {
        Ok(v) => v.values,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    // 前 N 根极值（不含当根，Donchian 经典口径）减/加 k·ATR。
    let hh = shift_right_1(&rolling_highest(high, TRAIL_N));
    let ll = shift_right_1(&rolling_lowest(low, TRAIL_N));
    let n = close.len();
    let mut trail_long = vec![f64::NAN; n];
    let mut trail_short = vec![f64::NAN; n];
    for i in 0..n {
        if hh.get(i).is_some_and(|v| v.is_finite())
            && atr_vals.get(i).is_some_and(|v| v.is_finite())
        {
            trail_long[i] = hh[i] - trailk * atr_vals[i];
        }
        if ll.get(i).is_some_and(|v| v.is_finite())
            && atr_vals.get(i).is_some_and(|v| v.is_finite())
        {
            trail_short[i] = ll[i] + trailk * atr_vals[i];
        }
    }
    Ok(bbrk_trail_signals(
        close,
        &bb.lower_band,
        &bb.upper_band,
        &trail_long,
        &trail_short,
    ))
}

impl Strategy for BbrkTrailStrategy {
    fn name(&self) -> &'static str {
        "BBRK_TRAIL"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, k) = self.pairs[idx];
        format!("sigma={s},trailk={k}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, k) = self.pairs[param_idx];
        compute(close, close, close, s, k)
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
        let (s, k) = self.pairs[param_idx];
        compute(high, low, close, s, k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_breakout_buys_trail_closes() {
        // 1.11 突破 Buy，之后 close 跌破追踪线 Close（中线 100 已被穿过但不再触发）。
        let close = vec![105.0, 111.0, 108.0, 102.0, 99.0];
        let lower = vec![90.0; 5];
        let upper = vec![110.0; 5];
        let trail_long = vec![100.0; 5];
        let trail_short = vec![90.0; 5];
        let sig = bbrk_trail_signals(&close, &lower, &upper, &trail_long, &trail_short);
        assert_eq!(sig[1], Signal::Buy);
        // close[2]=108>100 持有；close[4]=99<100 平仓（中线穿越不触发）。
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[4], Signal::Close);
    }

    #[test]
    fn test_midline_touch_no_longer_exits() {
        // 与 BBRK 的关键差异：碰中线但未破追踪线 → 继续持有。
        let close = vec![111.0, 100.0, 105.0];
        let lower = vec![90.0; 3];
        let upper = vec![110.0; 3];
        let trail_long = vec![95.0; 3];
        let trail_short = vec![90.0; 3];
        let sig = bbrk_trail_signals(&close, &lower, &upper, &trail_long, &trail_short);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Hold);
    }

    #[test]
    fn test_nan_trail_holds_position() {
        // 追踪线 NaN（预热期）→ 不误平仓，Hold 等待。
        let close = vec![111.0, 108.0];
        let lower = vec![90.0; 2];
        let upper = vec![110.0; 2];
        let trail_long = vec![f64::NAN, f64::NAN];
        let trail_short = vec![f64::NAN, f64::NAN];
        let sig = bbrk_trail_signals(&close, &lower, &upper, &trail_long, &trail_short);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkTrailStrategy {
            pairs: vec![(2.5, 3.0)],
        };
        assert_eq!(s.name(), "BBRK_TRAIL");
        assert_eq!(s.param_str(0), "sigma=2.5,trailk=3");
    }
}
