use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV3SlConfig;

/// 变种3止损闩锁版：入口同V3；下触逆势多加闩锁+止损，上触顺势多中线离场。
/// 闩锁：逆势单进场未武装，首根 close 穿过中线武装，武装后回穿平仓；
/// 止损：全程 low 触及 entry−k·ATR(进场根冻结) 平仓；ATR无效则止损禁用。
pub struct BbrkHmaV3SlStrategy {
    quads: Vec<(f64, usize, f64, usize)>,
}

impl BbrkHmaV3SlStrategy {
    /// 由 [`BbrkHmaV3SlConfig`] 构造; 参数网格由该配置展开。
    pub fn new(config: BbrkHmaV3SlConfig) -> Self {
        Self {
            quads: config.quads(),
        }
    }
}

/// 纯决策函数(可手算单测)。pos：1顺势多，2逆势多（闩锁+止损）。
#[allow(clippy::too_many_arguments)]
pub fn bbrk_hma_v3_sl_signals(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    atr_vals: &[f64],
    stopk: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(high.len())
        .min(low.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(atr_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    let mut armed = false;
    let mut entry = 0.0;
    let mut stopd = f64::INFINITY;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] {
                sig[i] = Signal::Buy;
                pos = 2;
                armed = false;
                entry = close[i];
                let ae = atr_vals[i];
                stopd = if ae.is_finite() && ae > 0.0 {
                    stopk * ae
                } else {
                    f64::INFINITY
                };
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
            if low[i] <= entry - stopd {
                sig[i] = Signal::Close;
                pos = 0;
                continue;
            }
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if !armed {
                if close[i] >= mid[i] {
                    armed = true;
                }
            } else if close[i] <= mid[i] {
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
    stopk: f64,
    atrlen: usize,
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
    let av = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(atrlen),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => vec![f64::NAN; close.len()],
    };
    Ok(bbrk_hma_v3_sl_signals(
        close,
        high,
        low,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &av,
        stopk,
    ))
}

impl Strategy for BbrkHmaV3SlStrategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V3_SL"
    }

    fn param_count(&self) -> usize {
        self.quads.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, k, a) = self.quads[idx];
        format!("sigma={s},bblen={b},stopk={k},atrlen={a}")
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
        let (s, b, k, a) = self.quads[param_idx];
        compute(high, low, close, s, b, k, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_contrarian_leg_latched_not_churned() {
        // 下触进多后次根仍在中线下：旧V3此处平仓，新逻辑持有。
        let (lower, mid, upper) = bands(4);
        let sig = bbrk_hma_v3_sl_signals(
            &[101.0, 89.0, 88.0, 102.0],
            &[101.0, 89.0, 88.0, 102.0],
            &[101.0, 89.0, 88.0, 102.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 4],
            2.0,
        );
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Hold); // 首穿武装，本根不平
    }

    #[test]
    fn test_trend_leg_unchanged_and_stop() {
        // 上触多中线离场原规则。
        let (lower, mid, upper) = bands(3);
        let sig = bbrk_hma_v3_sl_signals(
            &[101.0, 112.0, 100.0],
            &[101.0, 112.0, 100.0],
            &[101.0, 112.0, 100.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
        // 止损：entry89−10=79，low78触发。
        let sig2 = bbrk_hma_v3_sl_signals(
            &[101.0, 89.0, 80.0],
            &[101.0, 89.0, 80.0],
            &[101.0, 89.0, 78.0],
            &bands(3).0,
            &bands(3).1,
            &bands(3).2,
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig2[2], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaV3SlStrategy {
            quads: vec![(2.5, 20, 2.0, 50)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V3_SL");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,stopk=2,atrlen=50");
    }
}
