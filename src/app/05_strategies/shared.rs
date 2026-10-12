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
        if close[i - 1] < indicator[i - 1] && close[i] > indicator[i] {
            sig[i] = Signal::Buy;
        } else if close[i - 1] > indicator[i - 1] && close[i] < indicator[i] {
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

/// 通道右移一根(经典 Donchian/Aberration 口径: 用**前 N 根**的极值与
/// 当根收盘比较; 含当根的通道永远无法被穿越)。`out[0]=NaN`, 其余 `out[i]=v[i-1]`。
pub fn shift_right_1(v: &[f64]) -> Vec<f64> {
    let mut out = vec![f64::NAN; v.len()];
    if v.len() >= 2 {
        out[1..].copy_from_slice(&v[..v.len() - 1]);
    }
    out
}

/// UTC 自然日毫秒数。日内策略( Dual Thrust / R-Breaker / 菲阿里)按此切“交易日”。
pub const UTC_DAY_MS: i64 = 86_400_000;

/// bar 时间戳所属 UTC 日序号。
#[must_use]
pub fn utc_day_index(ts_ms: i64) -> i64 {
    ts_ms.div_euclid(UTC_DAY_MS)
}

/// O(n) 滑动最高/最低。`out[i]` 为 `data[i-n+1..=i]` 的极值, `i<n-1` 处为 NaN
/// (与 vector-ta 的 NaN 前缀惯例一致, 调用方统一按 NaN→Hold)。
/// Donchian / Aberration 通道用, n up to 数百、T up to 数百万均可。
pub fn rolling_highest(data: &[f64], n: usize) -> Vec<f64> {
    rolling_extreme(data, n, true)
}

pub fn rolling_lowest(data: &[f64], n: usize) -> Vec<f64> {
    rolling_extreme(data, n, false)
}

fn rolling_extreme(data: &[f64], n: usize, want_max: bool) -> Vec<f64> {
    use std::collections::VecDeque;
    let len = data.len();
    let mut out = vec![f64::NAN; len];
    if n == 0 || len < n {
        return out;
    }
    let mut dq: VecDeque<usize> = VecDeque::new();
    for i in 0..len {
        while let Some(&back) = dq.back() {
            let worse = if want_max {
                data[back] <= data[i]
            } else {
                data[back] >= data[i]
            };
            if worse {
                dq.pop_back();
            } else {
                break;
            }
        }
        dq.push_back(i);
        while let Some(&front) = dq.front() {
            if front + n <= i {
                dq.pop_front();
            } else {
                break;
            }
        }
        if i + 1 >= n {
            out[i] = data[*dq.front().expect("deque 非空")];
        }
    }
    out
}

/// f64 范围展开(6位小数取整, 避免 0.1 累加漂移)。上限 100000 档防爆。
pub fn expand_f64_range(start: f64, end: f64, step: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if !step.is_finite() || step <= 0.0 || !start.is_finite() || !end.is_finite() || start > end {
        return out;
    }
    let mut v = start;
    while v <= end + 1e-9 && out.len() < 100_000 {
        out.push((v * 1e6).round() / 1e6);
        v += step;
    }
    out
}

/// UTC 自然日聚合 bar(日内策略：Dual Thrust / R-Breaker / 菲阿里用)。
/// NaN 在极值折叠时跳过；`end_idx` 为当日最后一根 bar 下标(含)。
#[derive(Debug, Clone)]
pub struct DayBar {
    pub day: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub start_idx: usize,
    pub end_idx: usize,
}

pub fn aggregate_days(
    time: &[i64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
) -> Vec<DayBar> {
    let n = time
        .len()
        .min(open.len())
        .min(high.len())
        .min(low.len())
        .min(close.len());
    let mut days: Vec<DayBar> = Vec::new();
    for i in 0..n {
        let d = utc_day_index(time[i]);
        let (o, h, l, c) = (open[i], high[i], low[i], close[i]);
        match days.last_mut() {
            Some(cur) if cur.day == d => {
                if h.is_finite() && h > cur.high {
                    cur.high = h;
                }
                if l.is_finite() && l < cur.low {
                    cur.low = l;
                }
                if c.is_finite() {
                    cur.close = c;
                }
                cur.end_idx = i;
            }
            _ => days.push(DayBar {
                day: d,
                open: o,
                high: h,
                low: l,
                close: c,
                start_idx: i,
                end_idx: i,
            }),
        }
    }
    days
}

/// Money Flow Index（Bollinger Method II 确认用，官方口径：MFI 周期取带宽
/// 周期之半，BB20 → MFI10）。TP=(H+L+C)/3；MF=TP×V；N 根内 TP 上升根的 MF
/// 求和为 pos、下降根为 neg；MFI=100−100/(1+pos/neg)。
/// 首个完整窗不足（下标 <N）、窗内任一比较无效（非有限/量能<=0）、
/// pos==neg==0 → NaN，调用方统一按 NaN→Hold（P7）。O(n) 滑动窗。
pub fn money_flow_index(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    n: usize,
) -> Vec<f64> {
    let len = high.len().min(low.len()).min(close.len()).min(volume.len());
    let mut out = vec![f64::NAN; len];
    if n < 2 || len <= n {
        return out;
    }
    let tp: Vec<f64> = (0..len)
        .map(|i| {
            let (h, l, c, v) = (high[i], low[i], close[i], volume[i]);
            if h.is_finite() && l.is_finite() && c.is_finite() && v.is_finite() && v > 0.0 {
                (h + l + c) / 3.0
            } else {
                f64::NAN
            }
        })
        .collect();
    // 比较项 j≥1：(pos, neg, 无效标记)；out[i] 覆盖比较窗 j∈[i−n+1, i]。
    let mut pos_sum = 0.0;
    let mut neg_sum = 0.0;
    let mut bad = 0usize;
    let mut win: std::collections::VecDeque<(f64, f64, bool)> = std::collections::VecDeque::new();
    for i in 1..len {
        let entry = if tp[i].is_finite() && tp[i - 1].is_finite() {
            let mf = tp[i] * volume[i];
            if tp[i] > tp[i - 1] {
                (mf, 0.0, false)
            } else if tp[i] < tp[i - 1] {
                (0.0, mf, false)
            } else {
                (0.0, 0.0, false)
            }
        } else {
            (0.0, 0.0, true)
        };
        win.push_back(entry);
        pos_sum += entry.0;
        neg_sum += entry.1;
        if entry.2 {
            bad += 1;
        }
        if win.len() > n {
            let old = win.pop_front().expect("刚 push 后超窗，队非空");
            pos_sum -= old.0;
            neg_sum -= old.1;
            if old.2 {
                bad -= 1;
            }
        }
        if win.len() == n {
            out[i] = if bad > 0 {
                f64::NAN
            } else if neg_sum == 0.0 {
                if pos_sum > 0.0 { 100.0 } else { f64::NAN }
            } else {
                100.0 - 100.0 / (1.0 + pos_sum / neg_sum)
            };
        }
    }
    out
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
    fn test_rolling_extreme_matches_naive() {
        let data: Vec<f64> = vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let hi = rolling_highest(&data, 3);
        let lo = rolling_lowest(&data, 3);
        assert!(hi[0].is_nan() && hi[1].is_nan());
        assert_eq!(hi[2], 4.0);
        assert_eq!(hi[5], 9.0);
        assert_eq!(lo[2], 1.0);
        assert_eq!(lo[4], 1.0);
        assert_eq!(lo[7], 2.0);
        // n=1 恒为自身, n>len 全 NaN
        assert_eq!(rolling_highest(&data, 1), data);
        assert!(rolling_lowest(&data, 99).iter().all(|v| v.is_nan()));
    }

    #[test]
    fn test_expand_f64_range_no_drift() {
        assert_eq!(
            expand_f64_range(0.1, 0.6, 0.1),
            vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        );
        assert!(expand_f64_range(1.0, 0.5, 0.1).is_empty());
        assert!(expand_f64_range(1.0, 2.0, 0.0).is_empty());
    }

    #[test]
    fn test_shift_right_1() {
        assert_eq!(shift_right_1(&[1.0, 2.0, 3.0])[1], 1.0);
        assert!(shift_right_1(&[1.0, 2.0])[0].is_nan());
        assert!(shift_right_1(&[]).is_empty());
    }

    #[test]
    fn test_utc_day_index_splits_days() {
        assert_eq!(utc_day_index(0), 0);
        assert_eq!(utc_day_index(86_399_999), 0);
        assert_eq!(utc_day_index(86_400_000), 1);
    }

    #[test]
    fn test_aggregate_days_groups_bars() {
        // day0: 3 根, day1: 2 根(ms 时间戳)。
        let time = vec![0, 60_000, 120_000, 86_400_000, 86_460_000];
        let open = vec![10.0, 11.0, 12.0, 13.0, 14.0];
        let high = vec![11.0, 12.0, 13.0, 14.0, 15.0];
        let low = vec![9.0, 10.0, 11.0, 12.0, 13.0];
        let close = vec![10.5, 11.5, 12.5, 13.5, 14.5];
        let days = aggregate_days(&time, &open, &high, &low, &close);
        assert_eq!(days.len(), 2);
        assert_eq!(
            (days[0].open, days[0].high, days[0].low, days[0].close),
            (10.0, 13.0, 9.0, 12.5)
        );
        assert_eq!((days[0].start_idx, days[0].end_idx), (0, 2));
        assert_eq!((days[1].start_idx, days[1].end_idx), (3, 4));
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

    #[test]
    fn test_mfi_all_up_is_100() {
        // TP 严格递增、量恒定：neg==0 → 100；前 N 个下标为 NaN。
        let n = 10;
        let t: Vec<f64> = (0..15).map(|i| i as f64).collect();
        let v = vec![100.0; 15];
        let m = money_flow_index(&t, &t, &t, &v, n);
        assert_eq!(m.len(), 15);
        assert!(m[..n].iter().all(|x| !x.is_finite()));
        for x in &m[n..] {
            assert_eq!(*x, 100.0);
        }
    }

    #[test]
    fn test_mfi_all_down_is_0() {
        let n = 10;
        let t: Vec<f64> = (0..15).map(|i| (15 - i) as f64).collect();
        let v = vec![100.0; 15];
        let m = money_flow_index(&t, &t, &t, &v, n);
        assert!(m[..n].iter().all(|x| !x.is_finite()));
        for x in &m[n..] {
            assert_eq!(*x, 0.0);
        }
    }

    #[test]
    fn test_mfi_hand_computed() {
        // TP=[10,11,10]，V=100：j1 pos=1100，j2 neg=1000；
        // n=2 → 100−100/(1+1.1)=52.3809…。
        let t = vec![10.0, 11.0, 10.0];
        let v = vec![100.0; 3];
        let m = money_flow_index(&t, &t, &t, &v, 2);
        assert_eq!(m.len(), 3);
        assert!(!m[0].is_finite() && !m[1].is_finite());
        assert!((m[2] - 52.380_952_380_952_38).abs() < 1e-9, "m={m:?}");
    }

    #[test]
    fn test_mfi_bad_data_is_nan() {
        // 零量能 / NaN → 含该比较的窗全 NaN（P7，不静默）。
        let t = vec![10.0, 11.0, 12.0, 13.0];
        let z = vec![100.0, 0.0, 100.0, 100.0];
        let m = money_flow_index(&t, &t, &t, &z, 2);
        assert!(m.iter().all(|x| !x.is_finite()));
        let nan = vec![f64::NAN; 4];
        let m2 = money_flow_index(&nan, &nan, &nan, &vec![100.0; 4], 2);
        assert!(m2.iter().all(|x| !x.is_finite()));
    }
}
