use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::vwma::{VwmaInput, VwmaParams, vwma};

/// **VWMA** 策略实现。
pub struct VwmaStrategy {
    periods: Vec<usize>,
}

impl VwmaStrategy {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
    #[must_use]
    pub fn new(range: RangeParam<usize>) -> Self {
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for VwmaStrategy {
    fn name(&self) -> &'static str {
        "VWMA"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("vwma_{}", self.periods[idx])
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        _high: &[f64],
        _low: &[f64],
        close: &[f64],
        volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        if volume.len() != close.len() {
            return Err(crate::error::AppError::Msg(format!(
                "VWMA 需要与 close 等长的 volume 数据 (当前 volume={} close={}), 不做静默降级",
                volume.len(),
                close.len()
            )));
        }
        let input = VwmaInput::from_slice(
            close,
            volume,
            VwmaParams {
                period: Some(period),
            },
        );
        let ma = match vwma(&input) {
            Ok(v) => v.values,
            Err(e) => {
                return Err(crate::error::AppError::Msg(format!("VWMA 计算失败: {e}")));
            }
        };
        Ok(price_cross_signal(&ma, close))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::VwmaStrategy;
    use crate::config::RangeParam;
    use crate::strategies::{Signal, Strategy};

    #[test]
    fn test_generates_signals() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let volume: Vec<f64> = (0..200)
            .map(|i| 1.0 + (i as f64 * 0.1).sin().abs() * 5.0)
            .collect();
        let s = VwmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &volume, &[], 0)
            .unwrap();
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_rejects_volume_mismatch() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let s = VwmaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        assert!(
            s.generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
                .is_err()
        );
    }
}
