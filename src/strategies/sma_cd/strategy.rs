use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::config::MaGridCfg;
use crate::strategies::{Signal, Strategy, candle_direction_filter, price_cross_signal};

/// `sma` + K 线方向过滤。开仓额外要求当根 K 线方向与信号一致:
/// 多头需 `close > open`, 空头需 `close < open`, 十字星降级为 `Hold`。
pub struct SmaCdStrategy {
    periods: Vec<usize>,
}

impl SmaCdStrategy {
    /// 由 [`MaGridCfg`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: MaGridCfg) -> Self {
        let range = config.period.into_range();
        let periods: Vec<usize> = (range.start..=range.end).step_by(range.step).collect();
        Self { periods }
    }
}

impl Strategy for SmaCdStrategy {
    fn name(&self) -> &'static str {
        "SMA_CD"
    }

    fn param_count(&self) -> usize {
        self.periods.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("period={},variant=sma_cd", self.periods[idx])
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let period = self.periods[param_idx];
        let input = SmaInput::from_slice(
            bars.close(),
            SmaParams {
                period: Some(period),
            },
        );
        let ma = match sma(&input) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        let mut signals = price_cross_signal(&ma, bars.close());
        candle_direction_filter(&mut signals, bars.open(), bars.close());
        Ok(signals)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodParam, RangeParam};
    use crate::strategies::sma::SmaStrategy;

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

    fn cfg() -> MaGridCfg {
        MaGridCfg {
            period: PeriodParam::Range(RangeParam {
                start: 20,
                end: 20,
                step: 1,
            }),
        }
    }

    #[test]
    fn test_sma_cd_standard() {
        let s = SmaCdStrategy::new(cfg());
        assert_eq!(s.param_count(), 1);
        let close = prices(200);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        assert_eq!(sig.len(), 200);
    }

    #[test]
    fn test_sma_cd_signals_respect_candle_direction() {
        let s = SmaCdStrategy::new(cfg());
        let close = prices(400);
        let open = mixed_open(&close);
        let sig = s
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
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
    fn test_sma_cd_has_fewer_signals_than_sma() {
        let close = prices(400);
        let open = mixed_open(&close);
        let base = SmaStrategy::new(cfg().period.into_range())
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        let filtered = SmaCdStrategy::new(cfg())
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
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
    fn test_sma_cd_all_up_bars_keep_only_buys() {
        let close = prices(400);
        let open: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let sig = SmaCdStrategy::new(cfg())
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        assert!(!sig.contains(&Signal::Sell), "全阳线下不应出现 Sell 信号");
    }

    #[test]
    fn test_sma_cd_all_down_bars_keep_only_sells() {
        let close = prices(400);
        let open: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let sig = SmaCdStrategy::new(cfg())
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
            .unwrap();
        assert!(!sig.contains(&Signal::Buy), "全阴线下不应出现 Buy 信号");
    }

    #[test]
    fn test_sma_cd_param_str_records_variant() {
        assert_eq!(
            SmaCdStrategy::new(cfg()).param_str(0),
            "period=20,variant=sma_cd"
        );
    }

    /// **契约回归**: 数据短于 period 时**必须**返回 `ParamSkipped`,
    /// 而不是静默降级为全 Hold。
    ///
    /// 可触发场景: 旧实现返回 `Ok(vec![Hold; n])`, 于是这一档在报告里长得
    /// 和"零波动低风险"的真实结论**一模一样** —— 对参数网格过拟合研究
    /// (本项目的 PBO 模块)是最该被发现、却无人察觉的失败模式。
    #[test]
    fn test_sma_cd_short_data_is_skipped_not_silently_held() {
        let close = prices(5);
        let open = mixed_open(&close);
        let e = SmaCdStrategy::new(cfg())
            .generate_signals_full(
                crate::strategies::Bars::new(&open, &[], &[], &close, &[], &[]),
                0,
            )
            .expect_err("数据不足必须显式跳过, 不得静默降级");
        assert!(
            matches!(e, crate::error::AppError::ParamSkipped { .. }),
            "必须是 ParamSkipped, 实际: {e}"
        );
    }
}
