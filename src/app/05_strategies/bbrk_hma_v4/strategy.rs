use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkHmaV4Config;

/// 变种4：碰上下轨都做空；离场中线。
pub struct BbrkHmaV4Strategy {
    pairs: Vec<(f64, usize)>,
}

impl BbrkHmaV4Strategy {
    /// 由 [`BbrkHmaV4Config`] 构造; 参数网格由该配置展开。
    pub fn new(config: BbrkHmaV4Config) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_hma_v4_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] > upper[i] || close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
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

fn compute(close: &[f64], sigma: f64, bblen: usize) -> crate::error::Result<Vec<Signal>> {
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
    Ok(bbrk_hma_v4_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
    ))
}

impl Strategy for BbrkHmaV4Strategy {
    fn name(&self) -> &'static str {
        "BBRK_HMA_V4"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b) = self.pairs[idx];
        format!("sigma={s},bblen={b}")
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
        let (s, b) = self.pairs[param_idx];
        compute(close, s, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_either_touch_sells() {
        let (lower, mid, upper) = (vec![90.0; 4], vec![100.0; 4], vec![110.0; 4]);
        let sig = bbrk_hma_v4_signals(&[101.0, 89.0, 100.0, 112.0], &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Sell);
        assert_eq!(sig[2], Signal::Close);
        // i=3: 已平仓，碰上轨 → 再次 Sell。
        assert_eq!(sig[3], Signal::Sell);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkHmaV4Strategy {
            pairs: vec![(3.5, 25)],
        };
        assert_eq!(s.name(), "BBRK_HMA_V4");
        assert_eq!(s.param_str(0), "sigma=3.5,bblen=25");
    }
}
