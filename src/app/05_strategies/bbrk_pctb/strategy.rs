use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkPctbConfig;

/// BBRK+%b离场：flat + close>upper → Buy；flat + close<lower → Sell；
/// 持仓后看 %b（多头 %b<exitb 即 Close，空头 %b>1−exitb 即 Close）。
/// 带宽为零（upper==lower）或任一输入 NaN → Hold（P7，除零保护）。
/// exitb=0.5 时逐位等价于中线离场（BBRK），单测锁定。
pub struct BbrkPctbStrategy {
    pairs: Vec<(f64, f64)>,
}

impl BbrkPctbStrategy {
    /// 由 [`BbrkPctbConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkPctbConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
#[must_use]
pub fn bbrk_pctb_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    exitb: f64,
) -> Vec<Signal> {
    let _ = mid;
    let n = close.len().min(lower.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        let (c, lo, up) = (close[i], lower[i], upper[i]);
        if pos == 0 {
            if ![c, lo, up].iter().all(|v| v.is_finite()) {
                continue;
            }
            if c > up {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if c < lo {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else {
            if ![c, lo, up].iter().all(|v| v.is_finite()) || (up - lo).abs() < 1e-12 {
                continue;
            }
            let pctb = (c - lo) / (up - lo);
            if !pctb.is_finite() {
                continue;
            }
            if (pos == 1 && pctb < exitb) || (pos == -1 && pctb > 1.0 - exitb) {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(close: &[f64], sigma: f64, exitb: f64) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(20),
            devup: Some(sigma),
            devdn: Some(sigma),
            matype: Some("sma".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(_) => return Ok(vec![Signal::Hold; close.len()]),
    };
    Ok(bbrk_pctb_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        exitb,
    ))
}

impl Strategy for BbrkPctbStrategy {
    fn name(&self) -> &'static str {
        "BBRK_PCTB"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b) = self.pairs[idx];
        format!("sigma={s},exitb={b}")
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
        // %b 只需 close（与 BB 同源），双入口逐位一致。
        let (s, b) = self.pairs[param_idx];
        compute(close, s, b)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_breakout_buys_pctb_closes() {
        // lower=90, upper=110: 111 Buy(%b=1.05)；105 %b=0.75<0.8 Close。
        let close = vec![105.0, 111.0, 105.0, 100.0];
        let lower = vec![90.0; 4];
        let mid = vec![100.0; 4];
        let upper = vec![110.0; 4];
        let sig = bbrk_pctb_signals(&close, &lower, &mid, &upper, 0.8);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_exitb_half_equals_midline_exit() {
        // exitb=0.5 ⟺ close<mid：100 不走（%b=0.5 不小于 0.5），99 走。
        let close = vec![111.0, 100.0, 99.0];
        let lower = vec![90.0; 3];
        let mid = vec![100.0; 3];
        let upper = vec![110.0; 3];
        let sig = bbrk_pctb_signals(&close, &lower, &mid, &upper, 0.5);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_zero_bandwidth_holds() {
        // upper==lower 除零保护 → Hold 不崩。
        let close = vec![111.0, 100.0];
        let lower = vec![100.0; 2];
        let mid = vec![100.0; 2];
        let upper = vec![100.0; 2];
        let sig = bbrk_pctb_signals(&close, &lower, &mid, &upper, 0.8);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkPctbStrategy {
            pairs: vec![(2.5, 0.5)],
        };
        assert_eq!(s.name(), "BBRK_PCTB");
        assert_eq!(s.param_str(0), "sigma=2.5,exitb=0.5");
    }
}
