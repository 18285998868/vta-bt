use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::moving_averages::hma::{HmaInput, HmaParams, hma};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV1Config;

/// 变种1：HMA之上碰上下轨都做多，HMA之下只按原规则做空；离场中线。
pub struct BbrkHmaV1Strategy {
    triples: Vec<(f64, usize, usize)>,
}

impl BbrkHmaV1Strategy {
    /// 由 `BbrkHmaV1Config` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkHmaV1Config) -> Self {
        Self {
            triples: config.triples(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn bbrk_hma_v1_signals(
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
            if close[i] > hma_vals[i] {
                // 线上：碰上轨或碰下轨都做多。
                if close[i] > upper[i] || close[i] < lower[i] {
                    sig[i] = Signal::Buy;
                    pos = 1;
                }
            } else if close[i] < hma_vals[i] {
                // 线下：保持原规则（下破做空）。
                if close[i] < lower[i] {
                    sig[i] = Signal::Sell;
                    pos = -1;
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
    Ok(bbrk_hma_v1_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        &ma,
    ))
}

impl Strategy for BbrkHmaV1Strategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V1"
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
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, b, g) = self.triples[param_idx];
        compute(bars.close(), s, b, g)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_above_hma_upper_touch_buys() {
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sig = bbrk_hma_v1_signals(&[101.0, 112.0], &lower, &mid, &upper, &[105.0; 2]);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_above_hma_lower_touch_buys() {
        // 线上碰下轨（逆势多）：同样 Buy。
        let (lower, mid, upper) = (vec![90.0; 3], vec![100.0; 3], vec![110.0; 3]);
        let sig = bbrk_hma_v1_signals(&[101.0, 101.0, 89.0], &lower, &mid, &upper, &[85.0; 3]);
        assert_eq!(sig[2], Signal::Buy);
    }

    #[test]
    fn test_below_hma_keeps_original() {
        // 线下碰上轨：原规则无动作（只做空下破）。
        let (lower, mid, upper) = (vec![90.0; 2], vec![100.0; 2], vec![110.0; 2]);
        let sig = bbrk_hma_v1_signals(&[101.0, 112.0], &lower, &mid, &upper, &[115.0; 2]);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
        // 线下碰下轨： Sell。
        let sig2 = bbrk_hma_v1_signals(&[101.0, 89.0], &lower, &mid, &upper, &[95.0, 95.0]);
        assert_eq!(sig2[1], Signal::Sell);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaV1Strategy {
            triples: vec![(3.5, 25, 450)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V1");
        assert_eq!(s.param_str(0), "sigma=3.5,bblen=25,hmalen=450");
    }
}
