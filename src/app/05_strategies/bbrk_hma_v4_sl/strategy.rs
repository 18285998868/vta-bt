use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV4SlConfig;

/// 变种4止损闩锁版：入口同V4；上触逆势空加闩锁+止损，下触顺势空中线离场。
/// 闩锁：逆势单进场未武装，首根 close 穿过中线武装，武装后回穿平仓；
/// 止损：全程 high 触及 entry＋k·ATR(进场根冻结) 平仓；ATR无效则止损禁用。
pub struct BbrkHmaV4SlStrategy {
    quads: Vec<(f64, usize, f64, usize)>,
}

impl BbrkHmaV4SlStrategy {
    /// 由 [`BbrkHmaV4SlConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkHmaV4SlConfig) -> Self {
        Self {
            quads: config.quads(),
        }
    }
}

/// 纯决策函数(可手算单测)。pos：-1顺势空，-2逆势空（闩锁+止损）。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn bbrk_hma_v4_sl_signals(
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
            if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            } else if close[i] > upper[i] {
                sig[i] = Signal::Sell;
                pos = -2;
                armed = false;
                entry = close[i];
                let ae = atr_vals[i];
                stopd = if ae.is_finite() && ae > 0.0 {
                    stopk * ae
                } else {
                    f64::INFINITY
                };
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
            if close[i] >= entry + stopd {
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
        Err(e) => return Err(crate::strategies::param_skipped(e)),
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
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    Ok(bbrk_hma_v4_sl_signals(
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

impl Strategy for BbrkHmaV4SlStrategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V4_SL"
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

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_contrarian_leg_latched_not_churned() {
        let (lower, mid, upper) = bands(4);
        let sig = bbrk_hma_v4_sl_signals(
            &[101.0, 112.0, 113.0, 98.0],
            &[101.0, 112.0, 113.0, 98.0],
            &[101.0, 112.0, 113.0, 98.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 4],
            2.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Hold);
        assert_eq!(sig[3], Signal::Hold);
    }

    #[test]
    fn test_trend_leg_unchanged_and_stop() {
        let (lower, mid, upper) = bands(3);
        let sig = bbrk_hma_v4_sl_signals(
            &[101.0, 89.0, 100.0],
            &[101.0, 89.0, 100.0],
            &[101.0, 89.0, 100.0],
            &lower,
            &mid,
            &upper,
            &[5.0; 3],
            2.0,
        );
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Close);
        // 止损：entry112＋10=122，**close** 123 触发(收盘价判定, 非盘中 high)。
        let sig2 = bbrk_hma_v4_sl_signals(
            &[101.0, 112.0, 123.0],
            &[101.0, 112.0, 123.0],
            &[101.0, 112.0, 115.0],
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
        let s = BbrkHmaV4SlStrategy {
            quads: vec![(2.5, 20, 2.0, 50)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V4_SL");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,stopk=2,atrlen=50");
    }

    /// **契约**: ATR 算不出来时必须 `ParamSkipped`, 不得静默退化为无追踪止损。
    #[test]
    fn short_series_is_param_skipped() {
        let n = 8;
        let close: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let s = BbrkHmaV4SlStrategy {
            quads: vec![(2.0, 5, 2.0, 14)],
        };
        let err = s
            .generate_signals_full(&close, &high, &low, &close, &[], &[], 0)
            .expect_err("ATR(14) 在 8 根 bar 上不可计算");
        assert!(
            matches!(err, crate::error::AppError::ParamSkipped { .. }),
            "必须是 ParamSkipped, 实际: {err}"
        );
    }
}
