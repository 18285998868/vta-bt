use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy, rolling_highest, rolling_lowest, shift_right_1};

use super::config::BbrkDonConfig;

/// BBRK+Donchian极值离场：flat + close>upper → Buy；flat + close<lower → Sell；
/// 持仓后只看过去 N 根极值（多头 close < 近N根最低low(不含当根)即 Close，
/// 空头镜像）。中线触碰不再离场。NaN→Hold（P7，预热期不误平）。
pub struct BbrkDonStrategy {
    pairs: Vec<(f64, usize, usize)>,
}

impl BbrkDonStrategy {
    pub fn new(config: BbrkDonConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。`don_low/don_high` 为调用方算好的逐 bar
/// 过去N根最低low / 最高high（不含当根）。
pub fn bbrk_don_signals(
    close: &[f64],
    lower: &[f64],
    upper: &[f64],
    don_low: &[f64],
    don_high: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(upper.len())
        .min(don_low.len())
        .min(don_high.len());
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
            if ![close[i], don_low[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] < don_low[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if ![close[i], don_high[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > don_high[i] {
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
    bblen: usize,
    exitn: usize,
) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(bblen),
            devup: Some(sigma),
            devdn: Some(sigma),
            matype: Some("sma".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    let don_low = shift_right_1(&rolling_lowest(low, exitn));
    let don_high = shift_right_1(&rolling_highest(high, exitn));
    Ok(bbrk_don_signals(
        close,
        &bb.lower_band,
        &bb.upper_band,
        &don_low,
        &don_high,
    ))
}

impl Strategy for BbrkDonStrategy {
    fn name(&self) -> &'static str {
        "BBRK_DON"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, n) = self.pairs[idx];
        format!("sigma={s},bblen={b},exitn={n}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, b, n) = self.pairs[param_idx];
        compute(close, close, close, s, b, n)
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
        let (s, b, n) = self.pairs[param_idx];
        compute(high, low, close, s, b, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_breakout_buys_extreme_closes() {
        // 111 突破 Buy；don_low=100，close 跌破即 Close。
        let close = vec![105.0, 111.0, 108.0, 99.0];
        let lower = vec![90.0; 4];
        let upper = vec![110.0; 4];
        let don_low = vec![100.0; 4];
        let don_high = vec![120.0; 4];
        let sig = bbrk_don_signals(&close, &lower, &upper, &don_low, &don_high);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_midline_touch_no_longer_exits() {
        // 与 BBRK 的关键差异：回到中线附近但未破过去N根极值 → 继续持有。
        let close = vec![111.0, 100.0, 105.0];
        let lower = vec![90.0; 3];
        let upper = vec![110.0; 3];
        let don_low = vec![95.0; 3];
        let don_high = vec![120.0; 3];
        let sig = bbrk_don_signals(&close, &lower, &upper, &don_low, &don_high);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Hold);
    }

    #[test]
    fn test_nan_extreme_holds_position() {
        // 极值 NaN（预热期）→ 不误平仓。
        let close = vec![111.0, 108.0];
        let lower = vec![90.0; 2];
        let upper = vec![110.0; 2];
        let nan = vec![f64::NAN; 2];
        let sig = bbrk_don_signals(&close, &lower, &upper, &nan, &nan);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkDonStrategy {
            pairs: vec![(2.5, 20, 10)],
        };
        assert_eq!(s.name(), "BBRK_DON");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,exitn=10");
    }
}
