//! 参数稳定性: 边际分布、平台宽度与孤立尖峰。
//!
//! 只做**描述性**判断, 不把"参数平原"当成样本外证据。参数维度的邻域由
//! `strategy_params_json` 的键值对定义: 邻域 = 其它参数完全相同、该参数取
//! 相邻数值。字符串参数不参与邻域判断。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::stats::{mean, median, variance};

/// 参与稳定性分析的一行。
#[derive(Debug, Clone)]
pub(crate) struct StabilityRow {
    /// 参数 JSON(键 → 字符串值)。
    pub(crate) params_json: String,
    /// 参数串原文。
    pub(crate) params: String,
    /// fixed 腿年化夏普。
    pub(crate) fixed_sharpe: f64,
    /// fixed 腿总收益。
    pub(crate) fixed_return: f64,
}

/// 单个参数取值的边际统计。
#[derive(Debug, Clone, Serialize)]
pub struct ParamMarginal {
    /// 参数名。
    pub param: String,
    /// 参数值原文。
    pub value: String,
    /// 解析出的数值(非数值参数为 `None`)。
    pub numeric: Option<f64>,
    /// 行数。
    pub n: usize,
    /// 平均 fixed 夏普。
    pub mean_sharpe: f64,
    /// 中位 fixed 夏普。
    pub median_sharpe: f64,
    /// fixed 夏普样本标准差。
    pub std_sharpe: f64,
    /// 平均 fixed 总收益。
    pub mean_return: f64,
}

/// 组内参数稳定性摘要。
#[derive(Debug, Clone, Serialize)]
pub struct StabilitySummary {
    /// 最优参数串。
    pub best_params: Option<String>,
    /// 最优 fixed 夏普。
    pub best_sharpe: Option<f64>,
    /// 中位 fixed 夏普。
    pub median_sharpe: Option<f64>,
    /// 与最优夏普相差不超过 0.25 的行占比。
    pub share_within_tolerance: Option<f64>,
    /// 最优点的最好邻居夏普(其它参数不变, 该参数取相邻值)。
    pub neighbor_best_sharpe: Option<f64>,
    /// 最优点是否显著高于所有邻居。
    pub isolated_peak: bool,
    /// 参数边际。
    pub marginals: Vec<ParamMarginal>,
}

fn parse_params(s: &str) -> BTreeMap<String, String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn parse_num(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// 计算组内稳定性。`rows` 应为已按 `status == ok` 过滤的同一组。
pub(crate) fn stability_summary(rows: &[StabilityRow]) -> StabilitySummary {
    if rows.is_empty() {
        return StabilitySummary {
            best_params: None,
            best_sharpe: None,
            median_sharpe: None,
            share_within_tolerance: None,
            neighbor_best_sharpe: None,
            isolated_peak: false,
            marginals: Vec::new(),
        };
    }
    let sharpes: Vec<f64> = rows.iter().map(|r| r.fixed_sharpe).collect();
    let best_idx = sharpes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    let best = rows[best_idx].fixed_sharpe;
    let median_sharpe = median(&sharpes);
    let share = Some(
        sharpes
            .iter()
            .filter(|s| (best - **s).abs() <= 0.25)
            .count() as f64
            / rows.len() as f64,
    );

    let parsed: Vec<BTreeMap<String, String>> =
        rows.iter().map(|r| parse_params(&r.params_json)).collect();
    let best_map = &parsed[best_idx];
    let mut neighbor_best: Option<f64> = None;
    for (key, best_val) in best_map {
        let Some(best_num) = parse_num(best_val) else {
            continue;
        };
        let mut lower: Option<(f64, f64)> = None;
        let mut higher: Option<(f64, f64)> = None;
        for (i, map) in parsed.iter().enumerate() {
            if i == best_idx {
                continue;
            }
            let other_same = best_map
                .iter()
                .filter(|(k, _)| *k != key)
                .all(|(k, v)| map.get(k) == Some(v));
            if !other_same {
                continue;
            }
            let Some(num) = map.get(key).and_then(|v| parse_num(v)) else {
                continue;
            };
            if num < best_num {
                if lower.is_none_or(|(cur, _)| num > cur) {
                    lower = Some((num, rows[i].fixed_sharpe));
                }
            } else if num > best_num && higher.is_none_or(|(cur, _)| num < cur) {
                higher = Some((num, rows[i].fixed_sharpe));
            }
        }
        for cand in [lower.map(|(_, s)| s), higher.map(|(_, s)| s)]
            .into_iter()
            .flatten()
        {
            neighbor_best = Some(neighbor_best.map_or(cand, |cur: f64| cur.max(cand)));
        }
    }
    let isolated = neighbor_best.is_some_and(|nb| best > 0.5 && best - nb > 0.5);

    let mut by_param: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
    for (i, map) in parsed.iter().enumerate() {
        for (k, v) in map {
            by_param
                .entry(k.clone())
                .or_default()
                .entry(v.clone())
                .or_default()
                .push(i);
        }
    }
    let mut marginals = Vec::new();
    for (param, values) in by_param {
        let mut vals: Vec<(Option<f64>, String, Vec<usize>)> = values
            .into_iter()
            .map(|(value, idxs)| (parse_num(&value), value, idxs))
            .collect();
        vals.sort_by(|a, b| match (a.0, b.0) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => a.1.cmp(&b.1),
        });
        for (numeric, value, idxs) in vals {
            let s: Vec<f64> = idxs.iter().map(|&i| rows[i].fixed_sharpe).collect();
            let r: Vec<f64> = idxs.iter().map(|&i| rows[i].fixed_return).collect();
            let m = mean(&s);
            marginals.push(ParamMarginal {
                param: param.clone(),
                value,
                numeric,
                n: idxs.len(),
                mean_sharpe: m,
                median_sharpe: median(&s).unwrap_or(0.0),
                std_sharpe: variance(&s, m).map_or(0.0, f64::sqrt),
                mean_return: mean(&r),
            });
        }
    }

    StabilitySummary {
        best_params: Some(rows[best_idx].params.clone()),
        best_sharpe: Some(best),
        median_sharpe,
        share_within_tolerance: share,
        neighbor_best_sharpe: neighbor_best,
        isolated_peak: isolated,
        marginals,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn row(params: &str, json: &str, s: f64) -> StabilityRow {
        StabilityRow {
            params: params.into(),
            params_json: json.into(),
            fixed_sharpe: s,
            fixed_return: s / 10.0,
        }
    }

    #[test]
    fn finds_neighbors_and_isolated_peak() {
        let rows = vec![
            row("p=10", r#"{"p":"10"}"#, 0.2),
            row("p=20", r#"{"p":"20"}"#, 2.0),
            row("p=30", r#"{"p":"30"}"#, 0.1),
        ];
        let s = stability_summary(&rows);
        assert_eq!(s.best_params.as_deref(), Some("p=20"));
        assert!((s.neighbor_best_sharpe.unwrap() - 0.2).abs() < 1e-12);
        assert!(s.isolated_peak);
        assert_eq!(s.marginals.len(), 3);
    }

    #[test]
    fn plateau_is_not_isolated() {
        let rows = vec![
            row("p=10", r#"{"p":"10"}"#, 1.9),
            row("p=20", r#"{"p":"20"}"#, 2.0),
            row("p=30", r#"{"p":"30"}"#, 1.8),
        ];
        let s = stability_summary(&rows);
        assert!(!s.isolated_peak);
        assert!((s.share_within_tolerance.unwrap() - 1.0).abs() < 1e-12);
    }
}
