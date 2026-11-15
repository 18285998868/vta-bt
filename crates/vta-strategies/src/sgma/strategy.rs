use vector_ta::indicators::moving_averages::sgf::{SgfInput, SgfParams, sgf};

use crate::{Signal, Strategy, price_cross_signal};

use crate::params::{ParamSet, code};

use super::config::SgmaConfig;

/// SGMA 策略：价格穿越 Savitzky-Golay endpoint smoother。
pub struct SgmaStrategy {
    pairs: Vec<(usize, usize)>,
}

impl SgmaStrategy {
    /// 由 `SgmaConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: SgmaConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for SgmaStrategy {
    fn name(&self) -> &'static str {
        "SGMA"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_set(&self, idx: usize) -> ParamSet {
        let (p, o) = self.pairs[idx];
        let text = format!("sgma_p{p}_m{o}");
        ParamSet::builder(code::packed(&[(p as u64, 17), (o as u64, 17)]))
            .text(text)
            .kv("p", p)
            .kv("m", o)
            .build()
    }

    fn generate_signals_full(
        &self,
        bars: crate::Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let (period, poly_order) = self.pairs[param_idx];
        let ma = match sgf(&SgfInput::from_slice(
            bars.close(),
            SgfParams {
                period: Some(period),
                poly_order: Some(poly_order),
            },
        )) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::param_skipped(e)),
        };
        Ok(price_cross_signal(&ma, bars.close()))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PeriodParam;

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let cfg = SgmaConfig {
            period: PeriodParam::Single(21),
            poly_order: None,
        };
        let s = SgmaStrategy::new(cfg);
        assert_eq!(s.param_count(), 1);
        let sig = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &close, &[], &[]), 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
