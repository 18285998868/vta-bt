use crate::strategies::{
    PoleCount, Signal, Strategy, candle_direction_filter, price_cross_signal, supersmoother_3_pole,
};

use super::config::SsfStCdConfig;

/// `ssf_st` + K 线方向过滤。开仓额外要求当根 K 线方向与信号一致:
/// 多头需 `close > open`, 空头需 `close < open`, 十字星降级为 `Hold`。
pub struct SsfStCdStrategy {
    periods: Vec<usize>,
    poles: PoleCount,
}

impl SsfStCdStrategy {
    /// 由 `SsfStCdConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: SsfStCdConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self {
            periods,
            poles: config.poles,
        }
    }
}

/// 2 极 = 社区版 (无预平均): c1*Price[i] + b*SSF[i-1] - a2*SSF[i-2], 常数 sqrt(2)。
/// 与 `ssf_st` 共用同一实现, 保证除方向过滤外行为完全一致。
#[must_use]
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

impl Strategy for SsfStCdStrategy {
    fn name(&self) -> &'static str {
        "SSF_ST_CD"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!(
            "period={},poles={},variant=ssf_st_cd",
            self.periods[idx],
            usize::from(self.poles)
        )
    }

    fn generate_signals_full(
        &self,
        open: &[f64],
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
                None => {
                    return Err(crate::strategies::param_skipped(format!(
                        "SSF_ST_CD: 3 极超平滑器计算失败 (period={period})"
                    )));
                }
            },
        };
        let mut signals = price_cross_signal(&values, close);
        candle_direction_filter(&mut signals, open, close);
        Ok(signals)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};

    fn prices(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect()
    }

    fn mixed_open(close: &[f64]) -> Vec<f64> {
        close
            .iter()
            .enumerate()
            .map(|(i, c)| if i % 2 == 0 { c - 1.0 } else { c + 1.0 })
            .collect()
    }

    fn cfg(poles: PoleCount) -> SsfStCdConfig {
        SsfStCdConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles,
        }
    }

    #[test]
    fn test_ssf_st_cd_generates_signals() {
        let s = SsfStCdStrategy::new(cfg(PoleCount::Two));
        let close = prices(200);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_ssf_st_cd_three_pole() {
        let s = SsfStCdStrategy::new(cfg(PoleCount::Three));
        let close = prices(200);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_ssf_st_cd_signals_respect_candle_direction() {
        let s = SsfStCdStrategy::new(cfg(PoleCount::Two));
        let close = prices(400);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        for (i, &sg) in sig.iter().enumerate() {
            match sg {
                Signal::Buy => assert!(
                    close[i] > open[i],
                    "第 {i} 根 Buy 但 close({}) <= open({})",
                    close[i],
                    open[i]
                ),
                Signal::Sell => assert!(
                    close[i] < open[i],
                    "第 {i} 根 Sell 但 close({}) >= open({})",
                    close[i],
                    open[i]
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn test_ssf_st_cd_has_fewer_signals_than_ssf_st() {
        let close = prices(400);
        let open = mixed_open(&close);
        let base =
            crate::strategies::ssf_st::SsfStStrategy::new(crate::strategies::ssf_st::SsfStConfig {
                period: PeriodParam::Range(RangeParam {
                    start: 20,
                    end: 20,
                    step: 1,
                }),
                poles: PoleCount::Two,
            })
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        let filtered = SsfStCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        let cnt = |v: &[Signal]| v.iter().filter(|&&s| s != Signal::Hold).count();
        assert!(
            cnt(&filtered) <= cnt(&base),
            "过滤后信号数 {} 不应超过原版 {}",
            cnt(&filtered),
            cnt(&base)
        );
    }

    #[test]
    fn test_ssf_st_cd_two_pole_matches_ssf_st_before_filter() {
        // 2 极共用 ssf_filter: 全阳线(open=close-1)时只有 Buy 能通过,
        // 此时过滤后的 Buy 集合应与原版 Buy 集合一致
        let close = prices(400);
        let open: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let base =
            crate::strategies::ssf_st::SsfStStrategy::new(crate::strategies::ssf_st::SsfStConfig {
                period: PeriodParam::Range(RangeParam {
                    start: 20,
                    end: 20,
                    step: 1,
                }),
                poles: PoleCount::Two,
            })
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        let filtered = SsfStCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        let buys = |v: &[Signal]| {
            v.iter()
                .enumerate()
                .filter(|&(_, &s)| s == Signal::Buy)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        assert_eq!(buys(&filtered), buys(&base));
    }

    #[test]
    fn test_ssf_st_cd_all_down_bars_keep_only_sells() {
        let close = prices(400);
        let open: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let sig = SsfStCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(!sig.contains(&Signal::Buy), "全阴线下不应出现 Buy 信号");
    }

    #[test]
    fn test_ssf_st_cd_param_str_records_poles() {
        assert_eq!(
            SsfStCdStrategy::new(cfg(PoleCount::Two)).param_str(0),
            "period=20,poles=2,variant=ssf_st_cd"
        );
    }

    #[test]
    fn test_ssf_st_cd_short_data_holds() {
        let close = prices(5);
        let open = mixed_open(&close);
        let sig = SsfStCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.iter().all(|&x| x == Signal::Hold));
    }
}
