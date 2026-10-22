use vector_ta::indicators::moving_averages::ema::{EmaInput, EmaParams, ema};

use crate::strategies::{Signal, Strategy};

use super::config::MacdConfig;

/// MACD信号线穿越：`macd=EMAfast−EMAslow`，`sig=EMA(macd, sp)`，严格穿越。
/// 任一EMA失败(短序列/非法周期)时返回 `ParamSkipped`，**不再**静默全 Hold。
/// prev相等/NaN→Hold。
pub struct MacdStrategy {
    triples: Vec<(usize, usize, usize)>,
}

impl MacdStrategy {
    /// 由 [`MacdConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: MacdConfig) -> Self {
        Self {
            triples: config.triples(),
        }
    }
}

fn compute(
    close: &[f64],
    fast: usize,
    slow: usize,
    sigp: usize,
) -> crate::error::Result<Vec<Signal>> {
    let n = close.len();
    let mut sig = vec![Signal::Hold; n];
    let ef = match ema(&EmaInput::from_slice(
        close,
        EmaParams { period: Some(fast) },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let es = match ema(&EmaInput::from_slice(
        close,
        EmaParams { period: Some(slow) },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let macd: Vec<f64> = ef.iter().zip(es.iter()).map(|(a, b)| a - b).collect();
    let sg = match ema(&EmaInput::from_slice(
        &macd,
        EmaParams { period: Some(sigp) },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    for i in 1..n.min(sg.len()) {
        let (m0, m1, s0, s1) = (macd[i - 1], macd[i], sg[i - 1], sg[i]);
        if !(m0.is_finite() && m1.is_finite() && s0.is_finite() && s1.is_finite()) {
            continue;
        }
        if m0 < s0 && m1 > s1 {
            sig[i] = Signal::Buy;
        } else if m0 > s0 && m1 < s1 {
            sig[i] = Signal::Sell;
        }
    }
    Ok(sig)
}

impl Strategy for MacdStrategy {
    fn name(&self) -> &'static str {
        "MACD"
    }

    fn param_count(&self) -> usize {
        self.triples.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (f, s, g) = self.triples[idx];
        format!("macd_{f}_{s}_{g}")
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
        let (f, s, g) = self.triples[param_idx];
        compute(close, f, s, g)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_v_shape_gives_buy() {
        // 先跌后涨：慢线拐头处快线严格上穿信号线。
        let mut close: Vec<f64> = (0..150).map(|i| 200.0 - i as f64 * 0.5).collect();
        close.extend((0..150).map(|i| 125.0 + i as f64 * 0.5));
        let sig = compute(&close, 12, 26, 9).expect("足够长的数据必须可计算");
        assert!(sig.contains(&Signal::Buy));
    }

    #[test]
    fn test_inverse_v_gives_sell() {
        let mut close: Vec<f64> = (0..150).map(|i| 100.0 + i as f64 * 0.5).collect();
        close.extend((0..150).map(|i| 175.0 - i as f64 * 0.5));
        let sig = compute(&close, 12, 26, 9).expect("足够长的数据必须可计算");
        assert!(sig.contains(&Signal::Sell));
    }

    /// **契约**: 数据不足以算 EMA 时必须显式跳过, 不得静默降级为全 Hold。
    #[test]
    fn test_short_series_is_param_skipped() {
        let close = vec![100.0; 10];
        let err = compute(&close, 12, 26, 9).expect_err("短序列必须报错");
        assert!(
            matches!(err, crate::error::AppError::ParamSkipped { .. }),
            "必须是 ParamSkipped, 实际: {err}"
        );
        let s = MacdStrategy {
            triples: vec![(12, 26, 9)],
        };
        let err = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
            .expect_err("策略入口同样不得静默");
        assert!(matches!(err, crate::error::AppError::ParamSkipped { .. }));
    }

    #[test]
    fn test_name_and_params() {
        let s = MacdStrategy {
            triples: vec![(12, 26, 9)],
        };
        assert_eq!(s.name(), "MACD");
        assert_eq!(s.param_str(0), "macd_12_26_9");
    }
}
