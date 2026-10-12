use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::frama::{FramaInput, FramaParams, frama};

/// FRAMA 分形自适应均线: window 直接映射 period, sc=300/fc=1 取文献默认。
/// full 入口用真实 high/low; close-only 入口用 close 充当 high/low(口径说明: 各向同性时 D≈1→MA-style)。
pub struct FramaStrategy {
    periods: Vec<usize>,
}

impl FramaStrategy {
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for FramaStrategy {
    fn name(&self) -> &'static str {
        "FRAMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("frama_{}", self.periods[idx])
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        let ma = match frama(&FramaInput::from_slices(
            close,
            close,
            close,
            FramaParams {
                window: Some(period),
                sc: Some(300),
                fc: Some(1),
            },
        )) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(price_cross_signal(&ma, close))
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
        let period = self.periods[param_idx];
        let ma = match frama(&FramaInput::from_slices(
            high,
            low,
            close,
            FramaParams {
                window: Some(period),
                sc: Some(300),
                fc: Some(1),
            },
        )) {
            Ok(v) => v.values,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        Ok(price_cross_signal(&ma, close))
    }
}

#[cfg(test)]
mod tests {
    use super::FramaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = FramaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        assert_eq!(s.param_count(), 1);
        let sig = s.generate_signals(&[], &close, &[], 0).unwrap();
        assert!(sig.contains(&Signal::Buy));
    }
}
