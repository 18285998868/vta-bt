use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkBbConfig;

/// BBRK中轨对照：决策与 `bbrk_signals` 逐位一致，
/// BB 由 vector-ta 按 (period=len, matype=ma, dev=sigma) 直接算。
pub struct BbrkBbStrategy {
    triples: Vec<(String, usize, f64)>,
}

impl BbrkBbStrategy {
    /// 由 `BbrkBbConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkBbConfig) -> Self {
        Self {
            triples: config.triples(),
        }
    }
}

/// 纯决策函数（与 BBRK 同形，BB 由调用方供给）。
#[must_use]
pub fn bbrk_bb_signals(close: &[f64], lower: &[f64], mid: &[f64], upper: &[f64]) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        let vs = [close[i], lower[i], mid[i], upper[i]];
        if vs.iter().any(|v| !v.is_finite()) {
            continue;
        }
        if pos == 0 {
            if close[i] > upper[i] {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if close[i] < lower[i] {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if close[i] <= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if close[i] >= mid[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

fn compute(close: &[f64], ma: &str, len: usize, sigma: f64) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(len),
            devup: Some(sigma),
            devdn: Some(sigma),
            matype: Some(ma.to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    Ok(bbrk_bb_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
    ))
}

impl Strategy for BbrkBbStrategy {
    fn name(&self) -> &'static str {
        "BBRK_BB"
    }

    fn param_count(&self) -> usize {
        self.triples.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (m, l, s) = &self.triples[idx];
        format!("ma={m},len={l},sigma={s}")
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
        let (m, l, s) = &self.triples[param_idx];
        compute(close, m, *l, *s)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_break_upper_buys_mid_closes() {
        let close = vec![105.0, 111.0, 108.0, 100.0, 99.0];
        let lower = vec![90.0; 5];
        let mid = vec![100.0; 5];
        let upper = vec![110.0; 5];
        let sig = bbrk_bb_signals(&close, &lower, &mid, &upper);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkBbStrategy {
            triples: vec![("sma".to_string(), 20, 2.5)],
        };
        assert_eq!(s.name(), "BBRK_BB");
        assert_eq!(s.param_str(0), "ma=sma,len=20,sigma=2.5");
    }
}
