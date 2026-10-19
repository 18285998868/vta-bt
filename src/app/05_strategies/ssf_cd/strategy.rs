use crate::strategies::{
    PoleCount, Signal, Strategy, candle_direction_filter, price_cross_signal, supersmoother_3_pole,
};

use super::config::SsfCdConfig;

/// `ssf` + K 线方向过滤。开仓额外要求当根 K 线方向与信号一致:
/// 多头需 `close > open`, 空头需 `close < open`, 十字星降级为 `Hold`。
pub struct SsfCdStrategy {
    periods: Vec<usize>,
    poles: PoleCount,
}

impl SsfCdStrategy {
    /// 由 [`SsfCdConfig`] 构造; 参数网格由该配置展开。
    pub fn new(config: SsfCdConfig) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self {
            periods,
            poles: config.poles,
        }
    }
}

impl Strategy for SsfCdStrategy {
    fn name(&self) -> &'static str {
        "SSF_CD"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!(
            "period={},poles={},variant=ssf_cd",
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
        let mut signals = price_cross_signal(&values, close);
        candle_direction_filter(&mut signals, open, close);
        Ok(signals)
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

    /// open 取 close 的一个偏移, 使一部分 K 线为阳线、一部分为阴线。
    fn mixed_open(close: &[f64]) -> Vec<f64> {
        close
            .iter()
            .enumerate()
            .map(|(i, c)| if i % 2 == 0 { c - 1.0 } else { c + 1.0 })
            .collect()
    }

    fn cfg(poles: PoleCount) -> SsfCdConfig {
        SsfCdConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles,
        }
    }

    #[test]
    fn test_ssf_cd_standard() {
        let s = SsfCdStrategy::new(cfg(PoleCount::Two));
        assert!(s.param_count() > 0);
        let close = prices(200);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_ssf_cd_three_pole() {
        let s = SsfCdStrategy::new(cfg(PoleCount::Three));
        let close = prices(200);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_ssf_cd_signals_respect_candle_direction() {
        // 核心不变量: 每个非 Hold 信号的方向必须与 K 线方向一致
        let s = SsfCdStrategy::new(cfg(PoleCount::Two));
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
    fn test_ssf_cd_has_fewer_signals_than_ssf() {
        // 过滤只会减少信号, 不会增加
        let close = prices(400);
        let open = mixed_open(&close);
        let base = crate::strategies::ssf::SsfStrategy::new(crate::strategies::ssf::SsfConfig {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
            poles: PoleCount::Two,
        })
        .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
        .unwrap();
        let filtered = SsfCdStrategy::new(cfg(PoleCount::Two))
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
    fn test_ssf_cd_all_up_bars_keep_only_buys() {
        // open 全低于 close (全阳线): 只允许 Buy, Sell 必须被清空
        let close = prices(400);
        let open: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let sig = SsfCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(!sig.contains(&Signal::Sell), "全阳线下不应出现 Sell 信号");
    }

    #[test]
    fn test_ssf_cd_param_str_records_poles() {
        assert_eq!(
            SsfCdStrategy::new(cfg(PoleCount::Three)).param_str(0),
            "period=20,poles=3,variant=ssf_cd"
        );
    }

    #[test]
    fn test_ssf_cd_short_data_holds() {
        let close = prices(5);
        let open = mixed_open(&close);
        let sig = SsfCdStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(&open, &[], &[], &close, &[], &[], 0)
            .unwrap();
        assert!(sig.iter().all(|&x| x == Signal::Hold));
    }
}
