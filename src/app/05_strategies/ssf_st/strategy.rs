use crate::strategies::{PoleCount, Signal, Strategy, price_cross_signal, supersmoother_3_pole};

use super::config::SsfStConfig;

pub struct SsfStStrategy {
    periods: Vec<usize>,
    poles: PoleCount,
}

impl SsfStStrategy {
    pub fn new(config: SsfStConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self {
            periods,
            poles: config.poles,
        }
    }
}

/// 2 极 = 社区版 (无预平均): c1*Price[i] + b*SSF[i-1] - a2*SSF[i-2], 常数 sqrt(2)。
fn ssf_filter(close: &[f64], period: usize) -> Vec<f64> {
    use std::f64::consts::PI;
    let x = PI * 2.0_f64.sqrt() / period as f64;
    let a = (-x).exp();
    let a2 = a * a;
    let b = 2.0 * a * x.cos();
    let c1 = 1.0 + a2 - b;
    let n = close.len();
    let mut values = vec![0.0; n];
    if n > 0 {
        values[0] = close[0];
    }
    if n > 1 {
        values[1] = close[1];
    }
    for i in 2..n {
        values[i] = c1 * close[i] + b * values[i - 1] - a2 * values[i - 2];
    }
    values
}

impl Strategy for SsfStStrategy {
    fn name(&self) -> &'static str {
        "SSF_ST"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!(
            "period={},poles={},variant=ssf_st",
            self.periods[idx],
            usize::from(self.poles)
        )
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
        let period = self.periods[param_idx];
        let values = match self.poles {
            PoleCount::Two => ssf_filter(close, period),
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

    fn cfg(poles: PoleCount) -> SsfStConfig {
        SsfStConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles,
        }
    }

    #[test]
    fn test_ssf_st_generates_signals() {
        let s = SsfStStrategy::new(cfg(PoleCount::Two));
        let sig = s
            .generate_signals_full(&[], &[], &[], &prices(200), &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_ssf_st_three_pole() {
        let s = SsfStStrategy::new(cfg(PoleCount::Three));
        let sig = s
            .generate_signals_full(&[], &[], &[], &prices(200), &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_ssf_st_three_pole_matches_ssf_three_pole() {
        // 3 极在 ssf 与 ssf_st 之间共用同一实现, 信号必须逐根一致
        let close = prices(400);
        let a = SsfStStrategy::new(cfg(PoleCount::Three))
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        let b = crate::strategies::ssf::SsfStrategy::new(crate::strategies::ssf::SsfConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles: PoleCount::Three,
        })
        .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_ssf_st_two_and_three_pole_differ() {
        let close = prices(400);
        let two = SsfStStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        let three = SsfStStrategy::new(cfg(PoleCount::Three))
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_ne!(two, three, "2 极与 3 极不应产生完全相同的信号序列");
    }

    #[test]
    fn test_ssf_st_param_str_records_poles() {
        assert_eq!(
            SsfStStrategy::new(cfg(PoleCount::Two)).param_str(0),
            "period=20,poles=2,variant=ssf_st"
        );
    }
}
