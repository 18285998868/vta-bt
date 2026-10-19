use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV2SlConfig;

/// 变种2止损闩锁版：入口同V2；上触逆势空加闩锁+止损，其余腿原规则中线离场。
/// 闩锁：逆势单进场未武装，首根 close 穿过中线武装，武装后回穿平仓；
/// 止损：全程 high 触及 entry＋k·ATR(进场根冻结) 平仓；ATR无效则止损禁用。
pub struct BbrkHmaV2SlStrategy {
    quints: Vec<(f64, usize, usize, f64, usize)>,
}

impl BbrkHmaV2SlStrategy {
    /// 由 [`BbrkHmaV2SlConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkHmaV2SlConfig) -> Self {
        Self {
            quints: config.quints(),
        }
    }
}

/// 纯决策函数(可手算单测)。pos：1/-1顺势多空，-2逆势空（闩锁+止损）。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn bbrk_hma_v2_sl_signals(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    hma_vals: &[f64],
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
        .min(hma_vals.len())
        .min(atr_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    let mut armed = false;
    let mut entry = 0.0;
    let mut stopd = f64::INFINITY;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i], hma_vals[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            let c = close[i];
            if c < hma_vals[i] {
                if c < lower[i] {
                    sig[i] = Signal::Sell;
                    pos = -1;
                } else if c > upper[i] {
                    sig[i] = Signal::Sell;
                    pos = -2;
                    armed = false;
                    entry = c;
                    let ae = atr_vals[i];
                    stopd = if ae.is_finite() && ae > 0.0 {
                        stopk * ae
                    } else {
                        f64::INFINITY
                    };
                }
            } else if c > hma_vals[i] && c > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            }
        } else if pos == 1 {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] <= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if pos == -1 {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if high[i] >= entry + stopd {
                sig[i] = Signal::Close;
                pos = 0;
                continue;
            }
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if !armed {
                if close[i] <= mid[i] {
                    armed = true;
                }
            } else if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

#[allow(clippy::too_many_arguments)]
fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    bblen: usize,
    hmalen: usize,
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
    let ma = match hma(&HmaInput::from_slice(
        close,
        HmaParams {
            period: Some(hmalen),
        },
    )) {
        Ok(v) => v.values,
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
    Ok(bbrk_hma_v2_sl_signals(
        close,
        high,
        low,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &ma,
        &av,
        stopk,
    ))
}

impl Strategy for BbrkHmaV2SlStrategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V2_SL"
    }

    fn param_count(&self) -> usize {
        self.quints.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, g, k, a) = self.quints[idx];
        format!("sigma={s},bblen={b},hmalen={g},stopk={k},atrlen={a}")
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
        let (s, b, g, k, a) = self.quints[param_idx];
        compute(high, low, close, s, b, g, k, a)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_contrarian_short_no_immediate_exit() {
        // 线下碰上轨进空后，次根仍在中线之上：旧逻辑平仓，新逻辑持有。
        let (lower, mid, upper) = bands(3);
        let close = vec![101.0, 112.0, 113.0];
        let hma = vec![115.0; 3];
        let sig = bbrk_hma_v2_sl_signals(
            &close, &close, &close, &lower, &mid, &upper, &hma, &[5.0; 3], 2.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Hold);
    }

    #[test]
    fn test_latch_arm_then_exit_and_stop() {
        let (lower, mid, upper) = bands(4);
        let sig = bbrk_hma_v2_sl_signals(
            &[101.0, 112.0, 98.0, 101.0],
            &[101.0, 112.0, 98.0, 101.0],
            &[101.0, 112.0, 98.0, 101.0],
            &lower,
            &mid,
            &upper,
            &[115.0, 115.0, 110.0, 110.0],
            &[5.0; 4],
            2.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Close);
        // 止损：entry112＋10=122，high123触发。
        let sig2 = bbrk_hma_v2_sl_signals(
            &[101.0, 112.0, 115.0],
            &[101.0, 112.0, 123.0],
            &[101.0, 112.0, 115.0],
            &bands(3).0,
            &bands(3).1,
            &bands(3).2,
            &[115.0; 3],
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig2[1], Signal::Sell);
        assert_eq!(sig2[2], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaV2SlStrategy {
            quints: vec![(2.5, 20, 450, 2.0, 50)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V2_SL");
        assert_eq!(
            s.param_str(0),
            "sigma=2.5,bblen=20,hmalen=450,stopk=2,atrlen=50"
        );
    }
}
