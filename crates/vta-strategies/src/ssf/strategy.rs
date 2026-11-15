use crate::{PoleCount, Signal, Strategy, price_cross_signal, supersmoother_3_pole};

use crate::params::{ParamSet, code};

use super::config::SsfConfig;

/// **SSF** 策略实现。
pub struct SsfStrategy {
    periods: Vec<usize>,
    poles: PoleCount,
}

impl SsfStrategy {
    /// 由 `SsfConfig` 构造; 参数网格由该配置展开。
    #[must_use]
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

    fn param_set(&self, idx: usize) -> ParamSet {
        let p = self.periods[idx];
        let poles = usize::from(self.poles);
        let text = format!("period={p},poles={poles},variant=ssf");
        ParamSet::builder(code::packed(&[
            (p as u64, 20),
            (poles.saturating_sub(2) as u64, 1),
        ]))
        .text(text)
        .kv("period", p)
        .kv("poles", poles)
        .kv("variant", "ssf")
        .build()
    }

    fn generate_signals_full(
        &self,
        bars: crate::Bars<'_>,
        param_idx: usize,
    ) -> vta_core::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        // 2 极 = Ehlers 论文原版: c*(x[i] + x[i-1])/2 + c2*y[i-1] + c3*y[i-2], 常数 1.414
        // 3 极 = Cybernetic Analysis ch.13, 常数 1.738
        let values = match self.poles {
            PoleCount::Two => {
                use vector_ta::indicators::moving_averages::supersmoother::{
                    SuperSmootherInput, SuperSmootherParams, supersmoother,
                };

                match supersmoother(&SuperSmootherInput::from_slice(
                    bars.close(),
                    SuperSmootherParams {
                        period: Some(period),
                    },
                )) {
                    Ok(v) => v.values,
                    Err(e) => return Err(crate::param_skipped(e)),
                }
            }
            PoleCount::Three => match supersmoother_3_pole(bars.close(), period) {
                Some(v) => v,
                None => {
                    return Err(crate::param_skipped(format!(
                        "SSF: 3 极超平滑器计算失败 (period={period})"
                    )));
                }
            },
        };
        Ok(price_cross_signal(&values, bars.close()))
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
        let sig = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &prices(200), &[], &[]), 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_ssf_three_pole() {
        let s = SsfStrategy::new(cfg(PoleCount::Three));
        let sig = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &prices(200), &[], &[]), 0)
            .unwrap();
        assert_eq!(sig.len(), 200);
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_ssf_two_and_three_pole_differ() {
        let close = prices(400);
        let two = SsfStrategy::new(cfg(PoleCount::Two))
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &close, &[], &[]), 0)
            .unwrap();
        let three = SsfStrategy::new(cfg(PoleCount::Three))
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &close, &[], &[]), 0)
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

    /// **契约回归**: 数据长度小于 period 时**必须**返回 `ParamSkipped`,
    /// 而不是静默降级为全 Hold(见 `RunStatus::Invalid` 的文档)。
    #[test]
    fn test_ssf_short_data_is_skipped_not_silently_held() {
        let s = SsfStrategy::new(cfg(PoleCount::Two));
        let e = s
            .generate_signals_full(crate::Bars::new(&[], &[], &[], &prices(5), &[], &[]), 0)
            .expect_err("数据不足必须显式跳过, 不得静默降级");
        assert!(
            matches!(e, vta_core::error::AppError::ParamSkipped { .. }),
            "必须是 ParamSkipped, 实际: {e}"
        );
    }
}
