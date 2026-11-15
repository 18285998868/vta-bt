//! 日历收益: UTC 月/年聚合。

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;

/// 一个日历周期的收益。
#[derive(Debug, Clone, Serialize)]
pub struct PeriodReturn {
    /// 周期标签: `YYYY-MM` 或 `YYYY`。
    pub period: String,
    /// 周期起始时间戳(ms), 取该周期第一根 bar。
    pub start_time: i64,
    /// 周期结束时间戳(ms), 取该周期最后一根 bar。
    pub end_time: i64,
    /// 周期收益(小数)。首月以样本首点权益为基准。
    pub return_ratio: f64,
    /// 周期内 bar 数。
    pub bars: usize,
    /// 周期内最大回撤(小数, 从周期起点权益算)。
    pub max_drawdown: f64,
}

/// 月/年收益表。
#[derive(Debug, Clone, Serialize)]
pub struct CalendarSummary {
    /// 月度收益。
    pub monthly: Vec<PeriodReturn>,
    /// 年度收益。
    pub yearly: Vec<PeriodReturn>,
}

fn ts_to_utc(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(ms)
}

fn group_key(ts: i64, yearly: bool) -> Option<String> {
    let dt = ts_to_utc(ts)?;
    Some(if yearly {
        format!("{:04}", dt.year())
    } else {
        format!("{:04}-{:02}", dt.year(), dt.month())
    })
}

fn aggregate(times: &[i64], values: &[f64], yearly: bool) -> Vec<PeriodReturn> {
    if times.len() != values.len() || times.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<PeriodReturn> = Vec::new();
    let mut current_key: Option<String> = None;
    let mut start_time = times[0];
    let mut end_time = times[0];
    let mut start_equity = values[0];
    let mut last_equity = values[0];
    let mut peak = values[0];
    let mut max_dd = 0.0f64;
    let mut bars = 0usize;

    let flush = |out: &mut Vec<PeriodReturn>,
                 key: &str,
                 start_time: i64,
                 end_time: i64,
                 start_equity: f64,
                 last_equity: f64,
                 max_dd: f64,
                 bars: usize| {
        let ratio = if start_equity > 0.0 {
            last_equity / start_equity - 1.0
        } else {
            0.0
        };
        out.push(PeriodReturn {
            period: key.to_string(),
            start_time,
            end_time,
            return_ratio: ratio,
            bars,
            max_drawdown: max_dd,
        });
    };

    for (i, (&ts, &v)) in times.iter().zip(values.iter()).enumerate() {
        let Some(key) = group_key(ts, yearly) else {
            continue;
        };
        if current_key.as_deref() != Some(key.as_str()) {
            if let Some(prev) = current_key.take() {
                flush(
                    &mut out,
                    &prev,
                    start_time,
                    end_time,
                    start_equity,
                    last_equity,
                    max_dd,
                    bars,
                );
            }
            current_key = Some(key);
            start_time = ts;
            // 新周期的基准是上一周期末权益; 首个周期用样本首点。
            start_equity = if i == 0 { v } else { last_equity };
            peak = start_equity;
            max_dd = 0.0;
            bars = 0;
        }
        end_time = ts;
        last_equity = v;
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = (peak - v) / peak;
            max_dd = max_dd.max(dd);
        }
        bars += 1;
    }
    if let Some(key) = current_key {
        flush(
            &mut out,
            &key,
            start_time,
            end_time,
            start_equity,
            last_equity,
            max_dd,
            bars,
        );
    }
    out
}

/// 由 fixed 权益曲线生成月/年收益。
pub(crate) fn calendar_summary(times: &[i64], values: &[f64]) -> CalendarSummary {
    CalendarSummary {
        monthly: aggregate(times, values, false),
        yearly: aggregate(times, values, true),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn day_ms(y: i32, m: u32, d: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis())
            .unwrap_or_default()
    }

    #[test]
    fn monthly_and_yearly_returns() {
        // 2024-02 是闰年: 2/29 存在。
        let times = vec![
            day_ms(2023, 12, 31),
            day_ms(2024, 1, 31),
            day_ms(2024, 2, 29),
            day_ms(2024, 12, 31),
        ];
        let values = [100.0, 110.0, 121.0, 133.1];
        let c = calendar_summary(&times, &values);
        assert_eq!(c.yearly.len(), 2);
        assert_eq!(c.yearly[0].period, "2023");
        assert!(c.yearly[1].return_ratio > 0.20);
        assert_eq!(c.monthly.len(), 4);
        assert_eq!(c.monthly[1].period, "2024-01");
        assert!((c.monthly[1].return_ratio - 0.10).abs() < 1e-12);
        assert_eq!(c.monthly[2].period, "2024-02");
    }
}
