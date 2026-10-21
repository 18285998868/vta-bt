use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};
use vector_ta::indicators::moving_averages::linreg::{LinRegInput, LinRegParams, linreg};
use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::strategies::{ParamOutcome, Signal, Strategy, rolling_highest, rolling_lowest};

use super::config::BbrkTtmFireConfig;

/// Keltner 与 v1 同口径（EMA20 + k·ATR14），只改入口。
const KC_PERIOD: usize = 20;
const ATR_LEN: usize = 14;
/// 动量周期书中默认20，预注册固定（Donchian中轴/SMA/linreg三处同值）。
const MOM_LEN: usize = 20;

/// BBRK+TTM fire+动量版：flat + fire首根（当根squeeze_off且前根squeeze_on）
/// ＋ 动量同向（多头hist>0且rising，空头hist<0且falling）→ Buy/Sell；
/// 持仓后中线离场（与 BBRK/v1 相同）。任一输入 NaN → 不开仓（P7）。
pub struct BbrkTtmFireStrategy {
    pairs: Vec<(f64, usize, f64)>,
}

impl BbrkTtmFireStrategy {
    /// 由 [`BbrkTtmFireConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkTtmFireConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；`sqz`为当根squeeze_on，`hist`为动量柱)。
#[must_use]
pub fn bbrk_ttm_fire_signals(
    close: &[f64],
    mid: &[f64],
    sqz: &[bool],
    hist: &[f64],
) -> Vec<Signal> {
    let n = close.len().min(mid.len()).min(sqz.len()).min(hist.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], mid[i], hist[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            // fire首根：当根off且前根on（i==0无前根→不开）。
            if i == 0 || sqz[i] || !sqz[i - 1] {
                continue;
            }
            if !hist[i - 1].is_finite() {
                continue;
            }
            let rising = hist[i] > hist[i - 1];
            let falling = hist[i] < hist[i - 1];
            if hist[i] > 0.0 && rising {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if hist[i] < 0.0 && falling {
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

/// StockCharts口径动量柱：Donchian中轴(20)与SMA20取平均得重心，
/// delta=close−重心，linreg(20)平滑。NaN（预热）向上传。
#[must_use]
fn momentum(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let n = close.len();
    let hh = rolling_highest(high, MOM_LEN);
    let ll = rolling_lowest(low, MOM_LEN);
    let sma_vals = match sma(&SmaInput::from_slice(
        close,
        SmaParams {
            period: Some(MOM_LEN),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => return vec![f64::NAN; n],
    };
    let mut delta = vec![f64::NAN; n];
    for i in 0..n {
        if ![hh[i], ll[i], sma_vals[i], close[i]]
            .iter()
            .all(|v| v.is_finite())
        {
            continue;
        }
        delta[i] = close[i] - (hh[i] + ll[i]) / 4.0 - sma_vals[i] / 2.0;
    }
    match linreg(&LinRegInput::from_slice(
        &delta,
        LinRegParams {
            period: Some(MOM_LEN),
        },
    )) {
        Ok(v) => v.values,
        Err(_) => vec![f64::NAN; n],
    }
}

fn compute(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    bblen: usize,
    kc_mult: f64,
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
    let basis = match ema(&EmaInput::from_slice(
        close,
        EmaParams {
            period: Some(KC_PERIOD),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(ATR_LEN),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let n = close.len();
    let mut sqz = vec![false; n];
    for i in 0..n
        .min(bb.upper_band.len())
        .min(basis.len())
        .min(atr_vals.len())
    {
        let (up_b, lo_b, em, a) = (bb.upper_band[i], bb.lower_band[i], basis[i], atr_vals[i]);
        if ![up_b, lo_b, em, a].iter().all(|v| v.is_finite()) || a <= 0.0 {
            continue;
        }
        sqz[i] = up_b < em + kc_mult * a && lo_b > em - kc_mult * a;
    }
    let hist = momentum(high, low, close);
    Ok(bbrk_ttm_fire_signals(close, &bb.middle_band, &sqz, &hist))
}

impl Strategy for BbrkTtmFireStrategy {
    fn name(&self) -> &'static str {
        "BBRK_TTM_FIRE"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, k) = self.pairs[idx];
        format!("sigma={s},bblen={b},kc={k}")
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
        let (s, b, k) = self.pairs[param_idx];
        compute(high, low, close, s, b, k)
    }

    /// 批量版：动量柱/EMA基线/ATR14 与参数无关各算一次，
    /// BB 按 σ 去重，sqz 数组每 (σ,k) 现拼（O(n) 纯算术），再并行跑信号。
    /// 输出与逐个调逐位一致（单测锁定）。
    fn generate_signals_batch(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        range: std::ops::Range<usize>,
    ) -> crate::error::Result<Vec<ParamOutcome>> {
        use rayon::prelude::*;
        use std::collections::HashMap;
        use vector_ta::indicators::bollinger_bands::{
            BollingerBandsInput, BollingerBandsParams, bollinger_bands,
        };
        let hist = momentum(high, low, close);
        let basis: Option<Vec<f64>> = ema(&EmaInput::from_slice(
            close,
            EmaParams {
                period: Some(KC_PERIOD),
            },
        ))
        .ok()
        .map(|v| v.values);
        let atr_vals: Option<Vec<f64>> = atr(&AtrInput::from_slices(
            high,
            low,
            close,
            AtrParams {
                length: Some(ATR_LEN),
            },
        ))
        .ok()
        .map(|v| v.values);
        // 缓存键必须含 **BB 周期**: 周期现在也是网格维度, 只按 σ 建键会把
        // 不同周期的 BB 带**互相覆盖**(取到别的周期的带)。
        let mut bbmap: HashMap<(u64, usize), crate::strategies::CachedBands> = HashMap::new();
        for idx in range.clone() {
            let (s, b, _) = self.pairs[idx];
            bbmap.entry((s.to_bits(), b)).or_insert_with(|| {
                bollinger_bands(&BollingerBandsInput::from_slice(
                    close,
                    BollingerBandsParams {
                        period: Some(b),
                        devup: Some(s),
                        devdn: Some(s),
                        matype: Some("sma".to_string()),
                        devtype: Some(0),
                    },
                ))
                .ok()
                .map(|bb| (bb.lower_band, bb.middle_band, bb.upper_band))
            });
        }
        range
            .into_par_iter()
            .map(|idx| {
                let (s, b, k) = self.pairs[idx];
                let (lower, mid, upper, em, a_all) =
                    match (bbmap.get(&(s.to_bits(), b)), &basis, &atr_vals) {
                        (Some(Some((lo, m, up))), Some(em), Some(a_all)) => (lo, m, up, em, a_all),
                        _ => {
                            return Err(crate::strategies::param_skipped(format!(
                                "BBRK_TTM_FIRE: 共享指标缺失 (sigma={s}, bblen={b})"
                            )));
                        }
                    };
                let n = close.len().min(lower.len()).min(em.len()).min(a_all.len());
                let mut sqz = vec![false; n];
                for i in 0..n {
                    let a = a_all[i];
                    if ![upper[i], lower[i], em[i], a].iter().all(|v| v.is_finite()) || a <= 0.0 {
                        continue;
                    }
                    sqz[i] = upper[i] < em[i] + k * a && lower[i] > em[i] - k * a;
                }
                Ok(ParamOutcome::Signals(bbrk_ttm_fire_signals(
                    close, mid, &sqz, &hist,
                )))
            })
            .collect()
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn setup() -> (Vec<f64>, Vec<f64>, Vec<bool>, Vec<f64>) {
        // i=2为fire首根（sqz T,F,F...即sqz[1]=true,sqz[2]=false）。
        let close = vec![100.0, 100.0, 100.0, 100.0];
        let mid = vec![100.0; 4];
        let sqz = vec![false, true, false, false];
        (close, mid, sqz, vec![0.0; 4])
    }

    #[test]
    fn test_fire_with_rising_pos_hist_buys() {
        let (close, mid, sqz, _) = setup();
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[0.5, 1.0, 2.0, 3.0]);
        assert_eq!(sig[2], Signal::Buy);
    }

    #[test]
    fn test_fire_with_neg_hist_blocks_long() {
        let (close, mid, sqz, _) = setup();
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[-3.0, -2.0, -1.0, -0.5]);
        // hist<0且rising（−1>−2）→ 非空非多 → Hold。
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_fire_with_falling_hist_sells() {
        let (close, mid, sqz, _) = setup();
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[-0.5, -1.0, -2.0, -3.0]);
        assert_eq!(sig[2], Signal::Sell);
    }

    #[test]
    fn test_pos_but_flat_hist_holds() {
        let (close, mid, sqz, _) = setup();
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[1.0, 1.0, 1.0, 1.0]);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_still_squeezed_no_fire() {
        let close = vec![100.0, 100.0, 100.0];
        let mid = vec![100.0; 3];
        let sqz = vec![false, true, true];
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[1.0, 2.0, 3.0]);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_nan_hist_holds() {
        let (close, mid, sqz, _) = setup();
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[1.0, 2.0, f64::NAN, 3.0]);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_mid_exit_same_as_bbrk() {
        // 进多后回中线平仓。
        let close = vec![100.0, 100.0, 100.0, 99.0];
        let mid = vec![100.0; 4];
        let sqz = vec![false, true, false, false];
        let sig = bbrk_ttm_fire_signals(&close, &mid, &sqz, &[0.5, 1.0, 2.0, 3.0]);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_batch_matches_single() {
        let n = 60;
        let base: Vec<f64> = (0..n)
            .map(|i| 100.0 + (i as f64 * 0.2).sin() * 6.0)
            .collect();
        let open = base.clone();
        let high: Vec<f64> = base.iter().map(|v| v + 2.0).collect();
        let low: Vec<f64> = base.iter().map(|v| v - 2.0).collect();
        let close: Vec<f64> = base.iter().map(|v| v + 0.5).collect();
        let time: Vec<i64> = (0..n as i64).collect();
        let st = BbrkTtmFireStrategy {
            pairs: vec![(2.5, 20, 1.5), (2.5, 20, 2.0), (2.0, 20, 1.5)],
        };
        let batched = st
            .generate_signals_batch(&open, &high, &low, &close, &[], &time, 0..3)
            .unwrap();
        assert_eq!(batched.len(), 3);
        for (i, sig) in batched.iter().enumerate() {
            let sig = match sig {
                crate::strategies::ParamOutcome::Signals(s) => s,
                crate::strategies::ParamOutcome::Skipped(r) => {
                    panic!("BBRK_TTM_FIRE param {i} 不该被跳过: {r}")
                }
            };
            let one = st
                .generate_signals_full(&open, &high, &low, &close, &[], &time, i)
                .unwrap();
            assert_eq!(*sig, one, "param {i} 不一致");
        }
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkTtmFireStrategy {
            pairs: vec![(2.5, 20, 1.5)],
        };
        assert_eq!(s.name(), "BBRK_TTM_FIRE");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,kc=1.5");
    }
}
