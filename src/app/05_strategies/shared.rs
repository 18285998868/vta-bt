use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Hold,
    Buy,
    Sell,
    Close,
}

/// SuperSmoother 极数。仅支持 2 极与 3 极。
///
/// 序列化/反序列化沿用整数形式 (`2` / `3`), 非法值直接报错而非静默回落。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub enum PoleCount {
    #[default]
    Two,
    Three,
}

impl TryFrom<usize> for PoleCount {
    type Error = crate::error::AppError;

    fn try_from(poles: usize) -> Result<Self, Self::Error> {
        match poles {
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            other => Err(crate::error::AppError::Msg(format!(
                "poles 只支持 2 或 3, 当前为 {other}"
            ))),
        }
    }
}

impl From<PoleCount> for usize {
    fn from(poles: PoleCount) -> Self {
        match poles {
            PoleCount::Two => 2,
            PoleCount::Three => 3,
        }
    }
}

pub fn price_cross_signal(indicator: &[f64], close: &[f64]) -> Vec<Signal> {
    let n = close.len().min(indicator.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 1..n {
        if indicator[i].is_nan() || indicator[i - 1].is_nan() {
            continue;
        }
        if close[i - 1] < indicator[i - 1] && close[i] >= indicator[i] {
            sig[i] = Signal::Buy;
        } else if close[i - 1] > indicator[i - 1] && close[i] <= indicator[i] {
            sig[i] = Signal::Sell;
        }
    }
    sig
}

/// K 线方向过滤: 多头信号要求 `close > open`, 空头信号要求 `close < open`。
///
/// 用于 `ssf_cd` / `ssf_st_cd`: 趋势跟踪在震荡市反复假突破, 要求当根 K 线方向与
/// 信号方向一致, 可过滤掉逆势开仓。`close == open` (十字星) 一律降级为 `Hold`。
///
/// `open` / `close` 长度不足时, 该根降级为 `Hold`。方向未知时不开仓, 避免静默退化成
/// 未过滤的策略 (那会让回测结果看似变好, 实则过滤根本没生效)。
pub fn candle_direction_filter(signals: &mut [Signal], open: &[f64], close: &[f64]) {
    for (i, sig) in signals.iter_mut().enumerate() {
        if matches!(sig, Signal::Hold | Signal::Close) {
            continue;
        }
        let (Some(&o), Some(&c)) = (open.get(i), close.get(i)) else {
            *sig = Signal::Hold;
            continue;
        };
        let keep = match sig {
            Signal::Buy => c > o,
            Signal::Sell => c < o,
            _ => true,
        };
        if !keep {
            *sig = Signal::Hold;
        }
    }
}

/// Ehlers *Cybernetic Analysis for Stocks and Futures* ch.13 三极 SuperSmoother。
///
/// 2 极在 `ssf` (论文原版, 带预平均) 与 `ssf_st` (社区版, 无预平均) 之间分道扬镳,
/// 但 3 极两者共用同一实现, 故置于 shared 供两个策略调用。
///
/// 委托给 `vector-ta` 的 `supersmoother_3_pole`, 其余弦常数取 Ehlers 原式的 `1.738`。
/// 返回 `None` 表示数据长度不足以满足 `period`。
pub fn supersmoother_3_pole(close: &[f64], period: usize) -> Option<Vec<f64>> {
    use vector_ta::indicators::moving_averages::supersmoother_3_pole::{
        SuperSmoother3PoleInput, SuperSmoother3PoleParams, supersmoother_3_pole as ssf3,
    };

    ssf3(&SuperSmoother3PoleInput::from_slice(
        close,
        SuperSmoother3PoleParams {
            period: Some(period),
        },
    ))
    .ok()
    .map(|out| out.values)
}

/// 年化 bar 数。用于把逐 bar 收益换算成年化夏普/波动率。
///
/// **未列出的周期会静默落到 365, 造成年化结果成倍错误**, 故调用方必须在配置校验阶段
/// 用 [`is_known_timeframe`] 拦下未知周期。
pub fn bars_per_year(tf: &str) -> f64 {
    match tf {
        "1m" => 525_600.0,
        "3m" => 175_200.0,
        "5m" => 105_120.0,
        "15m" => 35_040.0,
        "30m" => 17_520.0,
        "1h" => 8_760.0,
        "2h" => 4_380.0,
        "4h" => 2_190.0,
        "6h" => 1_460.0,
        "8h" => 1_095.0,
        "12h" => 730.0,
        "1d" => 365.0,
        "1w" => 52.0,
        _ => 365.0,
    }
}

/// `bars_per_year` 是否认识该周期。配置校验用, 避免未知周期静默按 1d 年化。
pub fn is_known_timeframe(tf: &str) -> bool {
    matches!(
        tf,
        "1m" | "3m" | "5m" | "15m" | "30m" | "1h" | "2h" | "4h" | "6h" | "8h" | "12h" | "1d" | "1w"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_cross_buy_then_sell() {
        let close = vec![10.0, 12.0, 15.0, 14.0, 18.0];
        let indicator = vec![11.0, 13.0, 13.0, 15.0, 16.0];
        let sig = price_cross_signal(&indicator, &close);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Sell);
    }

    #[test]
    fn test_price_cross_all_hold_when_nan() {
        let close = vec![10.0, 11.0, 12.0];
        let indicator = vec![f64::NAN, f64::NAN, f64::NAN];
        let sig = price_cross_signal(&indicator, &close);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_candle_filter_keeps_agreeing_signals() {
        let open = vec![10.0, 12.0, 15.0];
        let close = vec![11.0, 13.0, 16.0];
        let mut sig = vec![Signal::Buy, Signal::Sell, Signal::Buy];
        candle_direction_filter(&mut sig, &open, &close);
        // 第 0/2 根涨, Buy 保留; 第 1 根涨, Sell 降级
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Buy);
    }

    #[test]
    fn test_candle_filter_doji_blocks_both_directions() {
        let open = vec![10.0, 10.0];
        let close = vec![10.0, 10.0]; // close == open
        let mut sig = vec![Signal::Buy, Signal::Sell];
        candle_direction_filter(&mut sig, &open, &close);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_candle_filter_preserves_hold_and_close() {
        let open = vec![10.0, 10.0];
        let close = vec![9.0, 11.0];
        let mut sig = vec![Signal::Hold, Signal::Close];
        candle_direction_filter(&mut sig, &open, &close);
        assert_eq!(sig[0], Signal::Hold);
        assert_eq!(sig[1], Signal::Close);
    }

    #[test]
    fn test_candle_filter_missing_open_blocks_signal() {
        // open 长度不足时不开仓: 否则过滤会静默失效, 回测看起来变好但实际没过滤
        let close = vec![10.0, 12.0];
        let open = vec![11.0];
        let mut sig = vec![Signal::Buy, Signal::Buy];
        candle_direction_filter(&mut sig, &open, &close);
        // 第 0 根有 open 且 close(10) <= open(11) -> Hold; 第 1 根缺 open -> Hold
        assert_eq!(sig[0], Signal::Hold);
        assert_eq!(sig[1], Signal::Hold);
    }

    #[test]
    fn test_candle_filter_handles_longer_open_slice() {
        // signals/close 之外的尾部不越界
        let close = vec![10.0, 12.0];
        let open = vec![9.0, 13.0, 1.0, 2.0];
        let mut sig = vec![Signal::Buy, Signal::Sell];
        candle_direction_filter(&mut sig, &open, &close);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Sell);
    }

    #[test]
    fn test_pole_count_try_from_accepts_only_two_and_three() {
        assert_eq!(PoleCount::try_from(2).unwrap(), PoleCount::Two);
        assert_eq!(PoleCount::try_from(3).unwrap(), PoleCount::Three);
        assert!(PoleCount::try_from(0).is_err());
        assert!(PoleCount::try_from(1).is_err());
        assert!(PoleCount::try_from(4).is_err());
    }

    #[test]
    fn test_pole_count_default_and_int_conversion() {
        assert_eq!(PoleCount::default(), PoleCount::Two);
        assert_eq!(usize::from(PoleCount::Two), 2);
        assert_eq!(usize::from(PoleCount::Three), 3);
    }

    #[test]
    fn test_supersmoother_3_pole_returns_full_length() {
        let close: Vec<f64> = (0..200)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 20.0)
            .collect();
        let values = supersmoother_3_pole(&close, 20).expect("3 极应可计算");
        assert_eq!(values.len(), close.len());
        assert!(values.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn test_supersmoother_3_pole_rejects_period_longer_than_data() {
        let close: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        assert!(supersmoother_3_pole(&close, 20).is_none());
    }

    #[test]
    fn test_bars_per_year() {
        assert_eq!(bars_per_year("1h"), 8760.0);
        assert_eq!(bars_per_year("1d"), 365.0);
        assert_eq!(bars_per_year("unknown"), 365.0);
    }

    #[test]
    fn test_bars_per_year_covers_all_known_timeframes() {
        // 数据目录里实际存在的 12 个周期, 一个都不能漏
        for tf in [
            "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d",
        ] {
            assert!(is_known_timeframe(tf), "{tf} 未被识别");
            // 年化 bar 数必须与周期长度成反比: 相邻周期比值 = 时长比
            assert!(bars_per_year(tf) > 52.0, "{tf} 年化 bar 数异常偏小");
        }
        assert!(is_known_timeframe("1w"));
        assert!(!is_known_timeframe("3s"));
        assert!(!is_known_timeframe(""));
    }

    #[test]
    fn test_bars_per_year_inverse_to_bar_duration() {
        // 1m 一年 = 60 * 24 * 365 = 525600; 每个周期按其分钟数等比缩放
        let minutes = |tf: &str| bars_per_year("1m") / bars_per_year(tf);
        assert_eq!(minutes("1m"), 1.0);
        assert_eq!(minutes("3m"), 3.0);
        assert_eq!(minutes("5m"), 5.0);
        assert_eq!(minutes("1h"), 60.0);
        assert_eq!(minutes("4h"), 240.0);
        assert_eq!(minutes("1d"), 1440.0);
    }
}
