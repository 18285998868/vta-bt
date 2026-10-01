use crate::strategies::{PoleCount, Signal, Strategy, price_cross_signal, supersmoother_3_pole};

use super::config::SsfConfig;

pub struct SsfStrategy {
    periods: Vec<usize>,
    poles: PoleCount,
}

impl SsfStrategy {
    pub fn new(config: SsfConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self {
            periods,
            poles: config.poles,
        }
    }
}

impl Strategy for SsfStrategy {
    fn name(&self) -> &'static str {
        "SSF"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!(
            "period={},poles={},variant=ssf",
            self.periods[idx],
            usize::from(self.poles)
        )
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        // 2 极 = Ehlers 论文原版: c*(x[i] + x[i-1])/2 + c2*y[i-1] + c3*y[i-2], 常数 1.414
        // 3 极 = Cybernetic Analysis ch.13, 常数 1.738
        let values = match self.poles {
            PoleCount::Two => {
                use vector_ta::indicators::moving_averages::supersmoother::{
                    SuperSmootherInput, SuperSmootherParams, supersmoother,
                };

                match supersmoother(&SuperSmootherInput::from_slice(
                    close,
                    SuperSmootherParams {
                        period: Some(period),
                    },
                )) {
                    Ok(v) => v.values,
                    Err(_) => return Ok(vec![Signal::Hold; close.len()]),
                }
            }
            PoleCount::Three => match supersmoother_3_pole(close, period) {
                Some(v) => v,
                None => return Ok(vec![Signal::Hold; close.len()]),
            },
        };
        Ok(price_cross_signal(&values, close))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};

    fn prices(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect()
    }

    fn cfg(poles: PoleCount) -> SsfConfig {
        SsfConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles,
        }
    }

    #[test]
    fn test_ssf_standard() {
        let s = SsfStrategy::new(cfg(PoleCount::Two));
        assert!(s.param_count() > 0);
        let sig = s.generate_signals(&[], &prices(200), &[], 0).unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_ssf_three_pole() {
        let s = SsfStrategy::new(cfg(PoleCount::Three));
        let sig = s.generate_signals(&[], &prices(200), &[], 0).unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_ssf_two_and_three_pole_differ() {
        let close = prices(400);
        let two = SsfStrategy::new(cfg(PoleCount::Two))
            .generate_signals(&[], &close, &[], 0)
            .unwrap();
        let three = SsfStrategy::new(cfg(PoleCount::Three))
            .generate_signals(&[], &close, &[], 0)
            .unwrap();
        assert_ne!(two, three, "2 极与 3 极不应产生完全相同的信号序列");
    }

    #[test]
    fn test_ssf_param_str_records_poles() {
        assert_eq!(
            SsfStrategy::new(cfg(PoleCount::Three)).param_str(0),
            "period=20,poles=3,variant=ssf"
        );
    }

    #[test]
    fn test_ssf_short_data_holds() {
        // 数据长度小于 period 时 crate 报错, 应降级为全 Hold 而非 panic
        let s = SsfStrategy::new(cfg(PoleCount::Two));
        let sig = s.generate_signals(&[], &prices(5), &[], 0).unwrap();
        assert!(sig.iter().all(|&x| x == Signal::Hold));
    }
}
