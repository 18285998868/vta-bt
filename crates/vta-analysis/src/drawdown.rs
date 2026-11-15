//! 回撤区间、恢复期与 Ulcer Index。
//!
//! 定义:
//! - 回撤 `dd_t = 1 - equity_t / peak_t`, 正数表示当前水下深度;
//! - episode 从创新高后的第一个下跌 bar 开始, 到下一次回到该高点(恢复)
//!   或样本结束为止;
//! - Ulcer Index = `√(mean(dd_t²))` (Martin & McCann 1989)。

/// 一次回撤区间。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DrawdownEpisode {
    /// 区间起点(前一个峰值)的时间戳(ms)。
    pub peak_time: i64,
    /// 最低点时间戳(ms)。
    pub trough_time: i64,
    /// 恢复时间戳(ms); 样本结束时仍未恢复则为 `None`。
    pub recovery_time: Option<i64>,
    /// 回撤深度(小数, 0.2 = 20%)。
    pub depth: f64,
    /// 峰值到谷底的 bar 数。
    pub peak_to_trough_bars: usize,
    /// 谷底到恢复的 bar 数; 未恢复为 `None`。
    pub recovery_bars: Option<usize>,
    /// 峰值到恢复(或样本结束)的总 bar 数。
    pub total_bars: usize,
}

/// 回撤汇总。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DrawdownSummary {
    /// 最大回撤(小数)。
    pub max_drawdown: f64,
    /// Ulcer Index(小数)。
    pub ulcer_index: f64,
    /// 水下时间占比(0~1)。
    pub time_in_drawdown: f64,
    /// 最长连续水下 bar 数。
    pub max_underwater_bars: usize,
    /// 样本结束时仍在水下的 bar 数。
    pub current_underwater_bars: usize,
    /// 按深度降序的 episode(最多 `max_episodes` 个)。
    pub episodes: Vec<DrawdownEpisode>,
}

/// 计算回撤汇总。`times` 与 `values` 必须等长且严格递增。
pub(crate) fn drawdown_summary(
    times: &[i64],
    values: &[f64],
    max_episodes: usize,
) -> Option<DrawdownSummary> {
    if values.len() < 2 || times.len() != values.len() {
        return None;
    }
    let mut peak = values[0];
    let mut peak_idx = 0usize;
    let mut trough = values[0];
    let mut trough_idx = 0usize;
    let mut underwater = false;
    let mut underwater_bars = 0usize;
    let mut max_underwater = 0usize;
    let mut underwater_count = 0usize;
    let mut sum_sq = 0.0;
    let mut episodes = Vec::new();

    // 第一个点本身不构成回撤; 从第二个点开始扫描。
    for i in 0..values.len() {
        let v = values[i];
        if !v.is_finite() {
            continue;
        }
        if v <= 0.0 {
            // 权益归零/转负时比例回撤已超过 100%; 记 100% 水下, 不参与峰值更新,
            // 但必须更新谷底, 否则深度会停在最后一次正权益处。
            if !underwater {
                underwater = true;
                underwater_bars = 0;
                trough = v;
                trough_idx = i;
            }
            underwater_bars += 1;
            max_underwater = max_underwater.max(underwater_bars);
            underwater_count += 1;
            sum_sq += 1.0;
            if v < trough {
                trough = v;
                trough_idx = i;
            }
            continue;
        }
        if i == 0 {
            continue;
        }
        if v >= peak {
            if underwater {
                episodes.push(DrawdownEpisode {
                    peak_time: times[peak_idx],
                    trough_time: times[trough_idx],
                    recovery_time: Some(times[i]),
                    depth: ((peak - trough) / peak).clamp(0.0, 1.0),
                    peak_to_trough_bars: trough_idx - peak_idx,
                    recovery_bars: Some(i - trough_idx),
                    total_bars: i - peak_idx,
                });
                underwater = false;
                underwater_bars = 0;
            }
            peak = v;
            peak_idx = i;
            trough = v;
            trough_idx = i;
            continue;
        }
        let dd = (peak - v) / peak;
        if dd > 0.0 {
            if !underwater {
                underwater = true;
                underwater_bars = 0;
                trough = v;
                trough_idx = i;
            }
            underwater_bars += 1;
            max_underwater = max_underwater.max(underwater_bars);
            underwater_count += 1;
            sum_sq += dd * dd;
            if v < trough {
                trough = v;
                trough_idx = i;
            }
        }
    }
    if underwater {
        episodes.push(DrawdownEpisode {
            peak_time: times[peak_idx],
            trough_time: times[trough_idx],
            recovery_time: None,
            depth: ((peak - trough) / peak).clamp(0.0, 1.0),
            peak_to_trough_bars: trough_idx - peak_idx,
            recovery_bars: None,
            total_bars: values.len() - peak_idx,
        });
    }

    let mut all_episodes = episodes;
    all_episodes.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    all_episodes.truncate(max_episodes);
    Some(DrawdownSummary {
        max_drawdown: all_episodes.first().map_or(0.0, |e| e.depth),
        ulcer_index: (sum_sq / values.len() as f64).sqrt(),
        time_in_drawdown: underwater_count as f64 / values.len() as f64,
        max_underwater_bars: max_underwater,
        current_underwater_bars: if underwater { underwater_bars } else { 0 },
        episodes: all_episodes,
    })
}

/// 尾部风险: 收益序列的 95% CVaR(Expected Shortfall)。
///
/// 取最差 5% 收益的均值; 样本不足 20 返回 `None`。值为负表示亏损。
pub(crate) fn cvar(rets: &[f64], tail: f64) -> Option<f64> {
    if rets.len() < 20 || !(0.0 < tail && tail < 1.0) {
        return None;
    }
    let mut v = rets.to_vec();
    v.sort_by(f64::total_cmp);
    let k = ((v.len() as f64 * tail).ceil() as usize).clamp(1, v.len());
    Some(v[..k].iter().sum::<f64>() / k as f64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hand_drawdown_episode() {
        let times: Vec<i64> = (0..7).map(|i| 1_000 + i * 10).collect();
        // 100 → 120 → 90 → 100 → 120 → 110 → 130
        let values = [100.0, 120.0, 90.0, 100.0, 120.0, 110.0, 130.0];
        let s = drawdown_summary(&times, &values, 10).unwrap();
        assert!((s.max_drawdown - 0.25).abs() < 1e-12);
        assert_eq!(s.episodes.len(), 2);
        assert_eq!(s.episodes[0].peak_time, 1_010);
        assert_eq!(s.episodes[0].trough_time, 1_020);
        assert_eq!(s.episodes[0].recovery_time, Some(1_040));
        assert_eq!(s.episodes[0].peak_to_trough_bars, 1);
        assert_eq!(s.episodes[0].recovery_bars, Some(2));
        assert_eq!(s.max_underwater_bars, 2);
    }

    #[test]
    fn unrecovered_drawdown_has_no_recovery() {
        let times = [0, 1, 2, 3];
        let values = [100.0, 110.0, 100.0, 90.0];
        let s = drawdown_summary(&times, &values, 10).unwrap();
        assert_eq!(s.episodes.len(), 1);
        assert!(s.episodes[0].recovery_time.is_none());
        assert_eq!(s.current_underwater_bars, 2);
    }

    #[test]
    fn negative_equity_is_capped_at_full_depth() {
        let times = [0, 1, 2, 3];
        let values = [100.0, 60.0, -10.0, 5.0];
        let s = drawdown_summary(&times, &values, 10).unwrap();
        assert_eq!(s.episodes.len(), 1);
        assert!((s.episodes[0].depth - 1.0).abs() < 1e-12);
        assert_eq!(s.episodes[0].trough_time, 2);
        assert_eq!(s.episodes[0].peak_to_trough_bars, 2);
        assert_eq!(s.current_underwater_bars, 3);
    }
}
