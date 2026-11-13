use vector_ta::indicators::moving_averages::alma::{AlmaInput, AlmaParams, alma};
use vector_ta::indicators::moving_averages::dema::{DemaInput, DemaParams, dema};
use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};
use vector_ta::indicators::moving_averages::kama::{KamaInput, KamaParams, kama};
use vector_ta::indicators::moving_averages::linreg::{LinRegInput, LinRegParams, linreg};
use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};
use vector_ta::indicators::moving_averages::tema::{TemaInput, TemaParams, tema};
use vector_ta::indicators::moving_averages::trima::{TrimaInput, TrimaParams, trima};
use vector_ta::indicators::moving_averages::wilders::{WildersInput, WildersParams, wilders};
use vector_ta::indicators::moving_averages::wma::{WmaInput, WmaParams, wma};
use vector_ta::indicators::moving_averages::zlema::{ZlemaInput, ZlemaParams, zlema};

use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkMaxConfig;

/// BBRK+通用均线 regime门：与 `bbrk_hma` 同决策逻辑（线上只多线下只空），
/// 只是 regime 线可换12种均线。σ固定2.5。
pub struct BbrkMaxStrategy {
    pairs: Vec<(String, usize, usize)>,
}

impl BbrkMaxStrategy {
    /// 由 `BbrkMaxConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkMaxConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

#[must_use]
fn ma_values(name: &str, close: &[f64], period: usize) -> Option<Vec<f64>> {
    let v = match name {
        "sma" => {
            sma(&SmaInput::from_slice(
                close,
                SmaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "wma" => {
            wma(&WmaInput::from_slice(
                close,
                WmaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "ema" => {
            ema(&EmaInput::from_slice(
                close,
                EmaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "wilders" => {
            wilders(&WildersInput::from_slice(
                close,
                WildersParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "dema" => {
            dema(&DemaInput::from_slice(
                close,
                DemaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "tema" => {
            tema(&TemaInput::from_slice(
                close,
                TemaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "trima" => {
            trima(&TrimaInput::from_slice(
                close,
                TrimaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "hma" => {
            hma(&HmaInput::from_slice(
                close,
                HmaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "zlema" => {
            zlema(&ZlemaInput::from_slice(
                close,
                ZlemaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "kama" => {
            kama(&KamaInput::from_slice(
                close,
                KamaParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        "alma" => {
            alma(&AlmaInput::from_slice(
                close,
                AlmaParams {
                    period: Some(period),
                    offset: None,
                    sigma: None,
                },
            ))
            .ok()?
            .values
        }
        "linreg" => {
            linreg(&LinRegInput::from_slice(
                close,
                LinRegParams {
                    period: Some(period),
                },
            ))
            .ok()?
            .values
        }
        _ => return None,
    };
    Some(v)
}

/// 纯决策函数(与 `bbrk_hma_signals` 逐位一致，regime 线由调用方供给)。
#[must_use]
pub fn bbrk_max_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    ma_vals: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(ma_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i], ma_vals[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            if close[i] > upper[i] && close[i] > ma_vals[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] && close[i] < ma_vals[i] {
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
        } else {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(
    close: &[f64],
    name: &str,
    len: usize,
    bblen: usize,
) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(bblen),
            devup: Some(2.5),
            devdn: Some(2.5),
            matype: Some("sma".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let ma = match ma_values(name, close, len) {
        Some(v) => v,
        None => {
            return Err(crate::strategies::param_skipped(format!(
                "BBRK_MAX: 未知 MA 名 `{name}`"
            )));
        }
    };
    Ok(bbrk_max_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &ma,
    ))
}

impl Strategy for BbrkMaxStrategy {
    fn name(&self) -> &'static str {
        "BBRK_MAX"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (m, l, b) = &self.pairs[idx];
        format!("ma={m},malen={l},bblen={b}")
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
        let (m, l, b) = &self.pairs[param_idx];
        compute(close, m, *l, *b)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_all_ma_names_compute() {
        // 12条线在普通序列上都算得出（不断言值，只断不断链）。
        let close: Vec<f64> = (0..300)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 5.0)
            .collect();
        for name in super::super::MA_NAMES {
            let v = ma_values(name, &close, 100);
            assert!(v.is_some(), "{name} 算不出");
            assert_eq!(v.unwrap().len(), close.len());
        }
    }

    #[test]
    fn test_unknown_ma_returns_none() {
        let close = vec![100.0; 10];
        assert!(ma_values("nope", &close, 5).is_none());
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkMaxStrategy {
            pairs: vec![("hma".to_string(), 100, 20)],
        };
        assert_eq!(s.name(), "BBRK_MAX");
        assert_eq!(s.param_str(0), "ma=hma,malen=100,bblen=20");
    }
}
