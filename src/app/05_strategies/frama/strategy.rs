use crate::config::RangeParam;
use crate::strategies::{Signal, Strategy, price_cross_signal};
use vector_ta::indicators::moving_averages::frama::{FramaInput, FramaParams, frama};

/// FRAMA 分形自适应均线: window 直接映射 period, sc=300/fc=1 取文献默认。
/// full 入口用真实 high/low; close-only 入口用 close 充当 high/low(口径说明: 各向同性时 D≈1→MA-style)。
pub struct FramaStrategy {
    periods: Vec<usize>,
}

impl FramaStrategy {
    /// 由均线周期范围 `range` 构造(单 MA 策略的公共入口)。
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
        // FRAMA 用真实 high/low(±1% 包络)计算; 引擎自 A2 起只走 full 入口。
        let high: Vec<f64> = close.iter().map(|v| v * 1.01).collect();
        let low: Vec<f64> = close.iter().map(|v| v * 0.99).collect();
        let sig = s
            .generate_signals_full(&[], &high, &low, &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), close.len());
        assert!(sig.contains(&Signal::Buy) || sig.contains(&Signal::Sell));
    }

    /// 回归: 历史上的 close-only 入口用 `close` 充当 high/low, 与 full 的真实
    /// high/low 结果**不同**; 两条路径并存正是 A2 要消除的分叉。现只剩 full。
    #[test]
    fn test_full_uses_real_high_low_not_close_proxy() {
        let n = 200;
        let close: Vec<f64> = (0..n)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let high: Vec<f64> = close.iter().map(|v| v * 1.5).collect();
        let low: Vec<f64> = close.iter().map(|v| v * 0.5).collect();
        let s = FramaStrategy::new(RangeParam {
            start: 20,
            end: 20,
            step: 1,
        });
        let full = s
            .generate_signals_full(&[], &high, &low, &close, &[], &[], 0)
            .unwrap();
        let close_as_hl = s
            .generate_signals_full(&[], &close, &close, &close, &[], &[], 0)
            .unwrap();
        assert_ne!(
            full, close_as_hl,
            "FRAMA 必须使用真实 high/low; 若与 close 代理结果相同说明退化成了旧口径"
        );
    }
}
