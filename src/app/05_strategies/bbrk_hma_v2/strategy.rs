use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV2Config;

/// 变种2：HMA之下碰上下轨都做空，HMA之上只按原规则做多；离场中线。
pub struct BbrkHmaV2Strategy {
    triples: Vec<(f64, usize, usize)>,
}

impl BbrkHmaV2Strategy {
    /// 由 [`BbrkHmaV2Config`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkHmaV2Config) -> Self {
        Self {
            triples: config.triples(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn bbrk_hma_v2_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    hma_vals: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(hma_vals.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i], hma_vals[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            if close[i] < hma_vals[i] {
                // 线下：碰下轨或碰上轨都做空。
                if close[i] < lower[i] || close[i] > upper[i] {
                    sig[i] = Signal::Sell;
                    pos = -1;
                }
            } else if close[i] > hma_vals[i] {
                // 线上：保持原规则（上破做多）。
                if close[i] > upper[i] {
                    sig[i] = Signal::Buy;
                    pos = 1;
                }
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
    sigma: f64,
    bblen: usize,
    hmalen: usize,
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
    let ma = match hma(&HmaInput::from_slice(
        close,
        HmaParams {
            period: Some(hmalen),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    Ok(bbrk_hma_v2_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &ma,
    ))
}

impl Strategy for BbrkHmaV2Strategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V2"
    }

    fn param_count(&self) -> usize {
        self.triples.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, g) = self.triples[idx];
        format!("sigma={s},bblen={b},hmalen={g}")
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
        let (s, b, g) = self.triples[param_idx];
        compute(close, s, b, g)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_below_hma_upper_touch_sells() {
        // 线下碰上轨（逆势空）：Sell。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sig = bbrk_hma_v2_signals(&[101.0, 112.0], &lower, &mid, &upper, &[115.0, 115.0]);
        assert_eq!(sig[1], Signal::Sell);
    }

    #[test]
    fn test_above_hma_keeps_original() {
        // 线上带内：Hold；线上碰上轨：Buy。
        let (lower, mid, upper) = (vec![90.0; 3], vec![100.0; 3], vec![110.0; 3]);
        let hma_vals = vec![95.0; 3];
        let sig = bbrk_hma_v2_signals(&[101.0, 101.0, 112.0], &lower, &mid, &upper, &hma_vals);
        assert_eq!(sig[0], Signal::Hold);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Buy);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaV2Strategy {
            triples: vec![(3.5, 25, 450)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V2");
        assert_eq!(s.param_str(0), "sigma=3.5,bblen=25,hmalen=450");
    }
}
