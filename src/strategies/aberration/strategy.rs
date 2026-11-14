use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};

use crate::strategies::{
    Signal, Strategy, price_cross_signal, rolling_highest, rolling_lowest, shift_right_1,
};

use super::config::AberrationConfig;

/// Aberration 波动率通道突破(思想级): 上轨=HH(N)+M·ATR(N)，
/// 下轨=LL(N)−M·ATR(N)，价格穿越翻仓，无日内平仓。NaN→Hold。
pub struct AberrationStrategy {
    pairs: Vec<(usize, f64)>,
}

impl AberrationStrategy {
    /// 由 `AberrationConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: AberrationConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for AberrationStrategy {
    fn name(&self) -> &'static str {
        "ABERRATION"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (n, m) = self.pairs[idx];
        format!("n={n},m={m}")
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (n, m) = self.pairs[param_idx];
        Self::compute(bars.high(), bars.low(), bars.close(), n, m)
    }
}

impl AberrationStrategy {
    fn compute(
        high: &[f64],
        low: &[f64],
        close: &[f64],
        n: usize,
        m: f64,
    ) -> crate::error::Result<Vec<Signal>> {
        let hh = rolling_highest(high, n);
        let ll = rolling_lowest(low, n);
        let atr_vals = match atr(&AtrInput::from_slices(
            high,
            low,
            close,
            AtrParams { length: Some(n) },
        )) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        let up: Vec<f64> = hh
            .iter()
            .zip(atr_vals.iter())
            .map(|(h, a)| {
                if h.is_finite() && a.is_finite() {
                    h + m * a
                } else {
                    f64::NAN
                }
            })
            .collect();
        let dn: Vec<f64> = ll
            .iter()
            .zip(atr_vals.iter())
            .map(|(l, a)| {
                if l.is_finite() && a.is_finite() {
                    l - m * a
                } else {
                    f64::NAN
                }
            })
            .collect();
        // 通道右移: 与前 N 根比较(经典口径, 含当根则永不穿越)。
        let up = shift_right_1(&up);
        let dn = shift_right_1(&dn);
        // 上穿上轨 Buy，下穿下轨 Sell(翻仓语义由引擎 positions 处理)。
        Ok(price_cross_signal(&up, close)
            .iter()
            .zip(price_cross_signal(&dn, close).iter())
            .map(|(u, d)| match (u, d) {
                (Signal::Buy, _) => Signal::Buy,
                (_, Signal::Sell) => Signal::Sell,
                _ => Signal::Hold,
            })
            .collect())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_jump_up_buys_no_sells() {
        // 微振荡基线(ATR>0 使带与价格分离)后跳涨: 跳涨根上穿上轨给 Buy。
        let mut close: Vec<f64> = (0..50).map(|i| 100.0 + (i % 2) as f64).collect();
        close.extend((0..50).map(|i| 130.0 + (i % 2) as f64));
        let sig = AberrationStrategy::compute(&close, &close, &close, 20, 2.0).unwrap();
        assert!(sig.contains(&Signal::Buy));
        assert!(!sig.contains(&Signal::Sell));
    }

    #[test]
    fn test_name() {
        let s = AberrationStrategy {
            pairs: vec![(20, 2.0)],
        };
        assert_eq!(s.name(), "ABERRATION");
        assert_eq!(s.param_str(0), "n=20,m=2");
    }
}
