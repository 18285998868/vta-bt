//! 输出层: `analysis.json`、`analysis.md` 与 `tables/*.csv`。
//!
//! 所有数值都直接取自 [`AnalysisReport`], 这里不做任何二次计算 ——
//! 否则 Markdown/CSV 与 JSON 可能给出互相矛盾的口径。

use std::fmt::Write as _;
use std::path::Path;

use chrono::{DateTime, Utc};
use vta_core::error::{AppError, Result};

use crate::model::{AnalysisReport, ComboAnalysis, GroupAnalysis, Significance, TradeStatsRow};

/// 写出完整分析产物。
///
/// 目录结构:
///
/// ```text
/// <out>/analysis.json
/// <out>/analysis.md
/// <out>/tables/combos.csv
/// <out>/tables/groups.csv
/// <out>/tables/significance.csv
/// <out>/tables/drawdowns.csv
/// <out>/tables/calendar.csv
/// <out>/tables/trades.csv
/// <out>/tables/param_marginals.csv
/// <out>/tables/exclusions.csv
/// <out>/tables/status.csv
/// ```
///
/// # Errors
/// 目录创建、JSON 序列化或文件写入失败时返回 `AppError`。
pub fn write_report(report: &AnalysisReport, out: &Path) -> Result<()> {
    let tables = out.join("tables");
    std::fs::create_dir_all(&tables)?;

    let json = serde_json::to_string_pretty(report)
        .map_err(|e| AppError::Msg(format!("analysis.json 序列化失败: {e}")))?;
    write_atomic(&out.join("analysis.json"), &format!("{json}\n"))?;
    write_atomic(&out.join("analysis.md"), &render_markdown(report))?;

    write_atomic(
        &tables.join("combos.csv"),
        &csv(&combo_rows(report), COMBO_HEADER),
    )?;
    write_atomic(
        &tables.join("groups.csv"),
        &csv(&group_rows(report), GROUP_HEADER),
    )?;
    write_atomic(
        &tables.join("significance.csv"),
        &csv(&significance_rows(report), SIGNIFICANCE_HEADER),
    )?;
    write_atomic(
        &tables.join("drawdowns.csv"),
        &csv(&drawdown_rows(report), DRAWDOWN_HEADER),
    )?;
    write_atomic(
        &tables.join("calendar.csv"),
        &csv(&calendar_rows(report), CALENDAR_HEADER),
    )?;
    write_atomic(
        &tables.join("trades.csv"),
        &csv(&trade_rows(report), TRADE_HEADER),
    )?;
    write_atomic(
        &tables.join("param_marginals.csv"),
        &csv(&marginal_rows(report), MARGINAL_HEADER),
    )?;
    write_atomic(
        &tables.join("exclusions.csv"),
        &csv(&exclusion_rows(report), EXCLUSION_HEADER),
    )?;
    write_atomic(
        &tables.join("status.csv"),
        &csv(&status_rows(report), &["status", "count"]),
    )?;

    Ok(())
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let mut tmp = path.to_path_buf();
    let name = tmp
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "analysis.tmp".to_string());
    tmp.set_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        AppError::Msg(format!(
            "{} -> {} 重命名失败: {e}",
            tmp.display(),
            path.display()
        ))
    })
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn render_markdown(r: &AnalysisReport) -> String {
    let mut out = String::with_capacity(16 * 1024);

    let _ = writeln!(out, "# 回测报告分析\n");
    let _ = writeln!(
        out,
        "由 `vta-analysis` 生成, JSON schema 版本 `{}`。\n",
        r.schema_version
    );
    let _ = writeln!(out, "- 工具版本: `{}`", r.tool_version);
    let _ = writeln!(out, "- 源码哈希: `{}`", r.source_hash);
    let _ = writeln!(out, "- 输入指纹: `{}`", r.input_digest);
    let _ = writeln!(out, "- 组合缓存: {}", r.input.detail_fixed);
    let _ = writeln!(out, "- 数据范围: {}", time_range(r));
    let _ = writeln!(
        out,
        "- 分析参数: alpha={}, bootstrap={}, block_len={}, seed={}, top={}, min_trades={}, trials={}",
        fmt_num(r.options.alpha, 4),
        r.options.bootstrap,
        r.options
            .block_len
            .map_or_else(|| "auto".to_string(), |v| v.to_string()),
        r.options.seed,
        r.options.top,
        r.options.min_trades,
        r.options
            .trials
            .map_or_else(|| "auto".to_string(), |v| v.to_string()),
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## 覆盖与状态\n");
    let _ = writeln!(out, "| 指标 | 值 |");
    let _ = writeln!(out, "|---|---|");
    let _ = writeln!(out, "| result.parquet 行数 | {} |", r.coverage.total_rows);
    let _ = writeln!(out, "| 状态 ok | {} |", r.coverage.ok_rows);
    let _ = writeln!(out, "| 可排名 | {} |", r.coverage.rank_eligible);
    let _ = writeln!(out, "| 可做显著性 | {} |", r.coverage.significance_eligible);
    let _ = writeln!(
        out,
        "| 缺 detail 的 ok 行 | {} |",
        r.coverage.detail_missing
    );
    let _ = writeln!(out, "| 对账不一致 | {} |", r.coverage.inconsistent);
    let _ = writeln!(out, "| 分组数 | {} |", r.coverage.groups);
    let _ = writeln!(out);
    if !r.coverage.status_counts.is_empty() {
        let _ = writeln!(out, "状态分布: {}", join_counts(r));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## 组合排名 Top-{}\n", r.top_ids.len());
    let top = top_combos(r);
    if top.is_empty() {
        let _ = writeln!(out, "没有满足条件的组合。\n");
    } else {
        let header = [
            "#",
            "品种",
            "周期",
            "策略",
            "参数",
            "fixed 夏普",
            "fixed 收益",
            "MaxDD",
            "笔数",
            "DSR",
            "BH q",
            "Bootstrap 95% CI",
        ];
        let rows: Vec<Vec<String>> = top
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (lo, hi) = c.significance.as_ref().map_or_else(
                    || ("-".to_string(), "-".to_string()),
                    |s| match &s.bootstrap {
                        Some(b) => (format!("{:.2}", b.ci_low), format!("{:.2}", b.ci_high)),
                        None => ("-".to_string(), "-".to_string()),
                    },
                );
                vec![
                    (i + 1).to_string(),
                    c.symbol.clone(),
                    c.timeframe.clone(),
                    c.strategy.clone(),
                    c.params.clone(),
                    fmt_num(c.fixed_sharpe, 3),
                    fmt_pct(c.fixed_return),
                    fmt_pct(c.fixed_max_drawdown),
                    c.trades.to_string(),
                    c.significance
                        .as_ref()
                        .and_then(|s| s.dsr)
                        .map_or_else(|| "-".to_string(), |v| fmt_num(v, 4)),
                    c.significance
                        .as_ref()
                        .and_then(|s| s.bh_q)
                        .map_or_else(|| "-".to_string(), |v| fmt_num(v, 4)),
                    format!("[{lo}, {hi}]"),
                ]
            })
            .collect();
        out.push_str(&table(&header, &rows));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## 分组概览\n");
    if r.groups.is_empty() {
        let _ = writeln!(out, "无分组。\n");
    } else {
        let header = [
            "品种",
            "周期",
            "策略",
            "方向",
            "窗口 bar",
            "行数",
            "可排名",
            "最优夏普",
            "中位夏普",
            "平台占比",
            "孤立尖峰",
            "PBO",
        ];
        let rows: Vec<Vec<String>> = r
            .groups
            .iter()
            .map(|g| {
                vec![
                    g.symbol.clone(),
                    g.timeframe.clone(),
                    g.strategy.clone(),
                    g.direction.clone(),
                    g.window_bars.to_string(),
                    g.rows.to_string(),
                    g.rank_eligible.to_string(),
                    opt_num(g.best_fixed_sharpe, 3),
                    opt_num(g.median_fixed_sharpe, 3),
                    g.stability
                        .share_within_tolerance
                        .map_or_else(|| "-".to_string(), fmt_pct),
                    if g.stability.isolated_peak {
                        "是".to_string()
                    } else {
                        "否".to_string()
                    },
                    g.pbo
                        .as_ref()
                        .map_or_else(|| "-".to_string(), |p| fmt_num(p.pbo, 4)),
                ]
            })
            .collect();
        out.push_str(&table(&header, &rows));
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "PBO 只在同组同时间网格内计算; `-` 表示曲线数或样本长度不足。\n"
        );
    }

    let _ = writeln!(out, "## Top-N 回撤区间\n");
    if r.drawdowns.is_empty() {
        let _ = writeln!(out, "没有可用的回撤区间(需要 detail fixed 曲线)。\n");
    } else {
        let header = [
            "名次",
            "参数",
            "峰值",
            "谷底",
            "恢复",
            "深度",
            "峰→谷 bar",
            "谷→恢复 bar",
        ];
        let rows: Vec<Vec<String>> = r
            .drawdowns
            .iter()
            .filter(|d| d.rank <= r.options.top.min(10))
            .map(|d| {
                vec![
                    d.rank.to_string(),
                    d.combo_id
                        .rsplit_once("|params=")
                        .map_or_else(|| d.combo_id.clone(), |(_, p)| p.to_string()),
                    fmt_ts(d.peak_time),
                    fmt_ts(d.trough_time),
                    d.recovery_time.map_or_else(|| "未恢复".to_string(), fmt_ts),
                    fmt_pct(d.depth),
                    d.peak_to_trough_bars.to_string(),
                    d.recovery_bars
                        .map_or_else(|| "-".to_string(), |v| v.to_string()),
                ]
            })
            .collect();
        out.push_str(&table(&header, &rows));
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "只列出有 detail 的组合; 名次是全量排名, 因此可能从 1 之后开始。\n"
        );
    }

    let with_detail = detail_top_combos(r);

    let _ = writeln!(out, "## 年度与月度收益(Top-3, 有 detail)\n");
    for c in with_detail.iter().take(3) {
        let yearly: Vec<_> = r
            .calendar
            .iter()
            .filter(|p| p.combo_id == c.id && p.period.len() == 4)
            .collect();
        if yearly.is_empty() {
            continue;
        }
        let _ = writeln!(
            out,
            "### {} / {} / {} `{}`\n",
            c.symbol, c.timeframe, c.strategy, c.params
        );
        let header = ["年度", "收益", "月内 MaxDD", "bar 数"];
        let rows: Vec<Vec<String>> = yearly
            .iter()
            .map(|p| {
                vec![
                    p.period.clone(),
                    fmt_pct(p.return_ratio),
                    fmt_pct(p.max_drawdown),
                    p.bars.to_string(),
                ]
            })
            .collect();
        out.push_str(&table(&header, &rows));
        let monthly: Vec<_> = r
            .calendar
            .iter()
            .filter(|p| p.combo_id == c.id && p.period.len() == 7)
            .collect();
        if !monthly.is_empty() {
            let header = ["月份", "收益", "月内 MaxDD", "bar 数"];
            let rows: Vec<Vec<String>> = monthly
                .iter()
                .map(|p| {
                    vec![
                        p.period.clone(),
                        fmt_pct(p.return_ratio),
                        fmt_pct(p.max_drawdown),
                        p.bars.to_string(),
                    ]
                })
                .collect();
            out.push_str(&table(&header, &rows));
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## 交易结构(Top-5, 有 detail)\n");
    let trade_top: Vec<&TradeStatsRow> = with_detail
        .iter()
        .take(5)
        .filter_map(|c| r.trade_stats.iter().find(|t| t.combo_id == c.id))
        .collect();
    if trade_top.is_empty() {
        let _ = writeln!(out, "没有逐笔成交文件。\n");
    } else {
        let header = [
            "参数",
            "笔数",
            "多/空",
            "胜率",
            "PF",
            "中位持仓",
            "MFE/MAE",
            "离场原因",
        ];
        let rows: Vec<Vec<String>> = trade_top
            .iter()
            .map(|t| {
                let s = &t.stats;
                let reasons = s
                    .exit_reasons
                    .iter()
                    .take(4)
                    .map(|x| format!("{}={}", x.name, x.count))
                    .collect::<Vec<_>>()
                    .join("; ");
                vec![
                    params_of(&t.combo_id),
                    s.n.to_string(),
                    format!("{}/{}", s.long, s.short),
                    s.win_rate.map_or_else(|| "-".to_string(), fmt_pct),
                    s.profit_factor
                        .map_or_else(|| "-".to_string(), |v| fmt_num(v, 3)),
                    s.median_bars_held
                        .map_or_else(|| "-".to_string(), |v| fmt_num(v, 1)),
                    s.edge_ratio
                        .map_or_else(|| "-".to_string(), |v| fmt_num(v, 3)),
                    reasons,
                ]
            })
            .collect();
        out.push_str(&table(&header, &rows));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## 参数边际(各组前 12 行)\n");
    if r.parameter_marginals.is_empty() {
        let _ = writeln!(out, "没有可用的参数边际(需要可排名组合)。\n");
    } else {
        let header = [
            "组",
            "参数",
            "取值",
            "n",
            "平均夏普",
            "中位夏普",
            "标准差",
            "平均收益",
        ];
        let mut rows: Vec<Vec<String>> = Vec::new();
        for g in &r.groups {
            for m in r
                .parameter_marginals
                .iter()
                .filter(|m| m.group_id == g.id)
                .take(12)
            {
                rows.push(vec![
                    short_group(g),
                    m.param.clone(),
                    m.value.clone(),
                    m.n.to_string(),
                    fmt_num(m.mean_sharpe, 3),
                    fmt_num(m.median_sharpe, 3),
                    fmt_num(m.std_sharpe, 3),
                    fmt_pct(m.mean_return),
                ]);
            }
        }
        out.push_str(&table(&header, &rows));
        let _ = writeln!(out, "完整边际见 `tables/param_marginals.csv`。\n");
    }

    let _ = writeln!(out, "## 排除与警告\n");
    let _ = writeln!(out, "- 硬排除: {} 条", r.exclusions.len());
    for (reason, count) in exclusion_groups(r) {
        let _ = writeln!(out, "  - `{reason}`: {count}");
    }
    if !r.warnings.is_empty() {
        let _ = writeln!(out, "- 警告: {} 条", r.warnings.len());
        for w in r.warnings.iter().take(20) {
            let _ = writeln!(out, "  - {w}");
        }
        if r.warnings.len() > 20 {
            let _ = writeln!(
                out,
                "  - ... 其余 {} 条见 analysis.json",
                r.warnings.len() - 20
            );
        }
    } else {
        let _ = writeln!(out, "- 警告: 0 条");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## 口径说明\n");
    let _ = writeln!(
        out,
        "- Sharpe 一律为逐 bar 对数收益、样本方差(ddof=1)、乘 `sqrt(bars_per_year)` 年化;\n\
         - PSR/DSR/MinTRL 用总体矩(偏度/原始峰度), 峰度正态基准为 3;\n\
         - DSR 的试验数默认取同组同窗口 `status == ok` 行数; 参数高度相关时该值偏保守;\n\
         - BH q 值只在同组内计算, 跨组混排没有 FDR 意义;\n\
         - 平稳 bootstrap 用固定 seed 与 Politis-White 自动块长, 同一输入逐位可复现;\n\
         - 回撤/日历/bootstrap 只覆盖有 detail 的 Top-N, 名次保留全量排名;\n\
         - PBO 只使用 `status == ok` 且共享同一时间网格的 fixed 曲线;\n\
         - 参数平台/孤立尖峰是描述性统计, 不是样本外证据。\n"
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## 产物清单\n");
    let _ = writeln!(out, "- `analysis.json`: 全部结构化结果(权威数据)");
    let _ = writeln!(out, "- `analysis.md`: 本文件");
    let _ = writeln!(out, "- `tables/*.csv`: 便于二次分析的扁平表");
    out
}

fn top_combos(r: &AnalysisReport) -> Vec<&ComboAnalysis> {
    r.top_ids
        .iter()
        .filter_map(|id| r.combos.iter().find(|c| &c.id == id))
        .collect()
}

fn detail_top_combos(r: &AnalysisReport) -> Vec<&ComboAnalysis> {
    r.detail_top_ids
        .iter()
        .filter_map(|id| r.combos.iter().find(|c| &c.id == id))
        .collect()
}

// ---------------------------------------------------------------------------
// CSV 行
// ---------------------------------------------------------------------------

const COMBO_HEADER: &[&str] = &[
    "id",
    "group_id",
    "symbol",
    "timeframe",
    "strategy",
    "params",
    "direction",
    "fee_bps",
    "slippage_bps",
    "delay_bars",
    "fill_close",
    "window_bars",
    "status",
    "trades",
    "bars_used",
    "rank_eligible",
    "significance_eligible",
    "fixed_return",
    "fixed_sharpe",
    "fixed_max_drawdown",
    "fixed_ulcer",
    "fixed_cvar_95",
    "fixed_worst_bar",
    "compound_return",
    "compound_sharpe",
    "compound_max_drawdown",
    "compound_ulcer",
    "compound_cvar_95",
    "reconciliation_checked",
    "reconciliation_consistent",
    "psr_zero",
    "dsr",
    "bh_q",
    "bootstrap_ci_low",
    "bootstrap_ci_high",
];

fn combo_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.combos
        .iter()
        .map(|c| {
            let s = c.significance.as_ref();
            let b = s.and_then(|x| x.bootstrap.as_ref());
            vec![
                c.id.clone(),
                c.group_id.clone(),
                c.symbol.clone(),
                c.timeframe.clone(),
                c.strategy.clone(),
                c.params.clone(),
                c.direction.clone(),
                c.fee_bps.to_string(),
                c.slippage_bps.to_string(),
                c.delay_bars.to_string(),
                c.fill_close.to_string(),
                c.window_bars.to_string(),
                c.status.clone(),
                c.trades.to_string(),
                c.bars_used.to_string(),
                c.rank_eligible.to_string(),
                c.significance_eligible.to_string(),
                c.fixed_return.to_string(),
                c.fixed_sharpe.to_string(),
                c.fixed_max_drawdown.to_string(),
                opt_f(c.fixed_ulcer),
                opt_f(c.fixed_cvar_95),
                opt_f(c.fixed_worst_bar),
                c.compound_return.to_string(),
                c.compound_sharpe.to_string(),
                c.compound_max_drawdown.to_string(),
                opt_f(c.compound_ulcer),
                opt_f(c.compound_cvar_95),
                c.reconciliation.checked.to_string(),
                c.reconciliation.consistent.to_string(),
                opt_f(s.and_then(|x| x.psr_zero)),
                opt_f(s.and_then(|x| x.dsr)),
                opt_f(s.and_then(|x| x.bh_q)),
                opt_f(b.map(|x| x.ci_low)),
                opt_f(b.map(|x| x.ci_high)),
            ]
        })
        .collect()
}

const GROUP_HEADER: &[&str] = &[
    "id",
    "symbol",
    "timeframe",
    "strategy",
    "direction",
    "fee_bps",
    "slippage_bps",
    "delay_bars",
    "fill_close",
    "window_bars",
    "rows",
    "ok_rows",
    "rank_eligible",
    "significance_eligible",
    "best_params",
    "best_fixed_sharpe",
    "median_fixed_sharpe",
    "share_positive",
    "plateau_share",
    "neighbor_best_sharpe",
    "isolated_peak",
    "pbo",
    "pbo_t_bars",
    "pbo_s_blocks",
];

fn group_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.groups
        .iter()
        .map(|g| {
            vec![
                g.id.clone(),
                g.symbol.clone(),
                g.timeframe.clone(),
                g.strategy.clone(),
                g.direction.clone(),
                g.fee_bps.to_string(),
                g.slippage_bps.to_string(),
                g.delay_bars.to_string(),
                g.fill_close.to_string(),
                g.window_bars.to_string(),
                g.rows.to_string(),
                g.ok_rows.to_string(),
                g.rank_eligible.to_string(),
                g.significance_eligible.to_string(),
                g.best_params.clone().unwrap_or_default(),
                opt_f(g.best_fixed_sharpe),
                opt_f(g.median_fixed_sharpe),
                opt_f(g.share_positive),
                opt_f(g.stability.share_within_tolerance),
                opt_f(g.stability.neighbor_best_sharpe),
                g.stability.isolated_peak.to_string(),
                opt_f(g.pbo.as_ref().map(|p| p.pbo)),
                g.pbo
                    .as_ref()
                    .map_or_else(String::new, |p| p.t_bars.to_string()),
                g.pbo
                    .as_ref()
                    .map_or_else(String::new, |p| p.s_blocks.to_string()),
            ]
        })
        .collect()
}

const SIGNIFICANCE_HEADER: &[&str] = &[
    "combo_id",
    "n_returns",
    "sr_bar",
    "sr_annual",
    "skew",
    "kurtosis",
    "lo_se_annual",
    "psr_zero",
    "psr_p_value",
    "min_trl_bars",
    "min_trl_ok",
    "trials",
    "trial_sr_variance",
    "expected_max_sr_annual",
    "dsr",
    "bh_q",
    "bh_group_size",
    "bootstrap_block_len",
    "bootstrap_resamples",
    "bootstrap_mean_sr",
    "bootstrap_se_sr",
    "bootstrap_ci_low",
    "bootstrap_ci_high",
];

fn significance_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.combos
        .iter()
        .filter_map(|c| {
            let s = c.significance.as_ref()?;
            Some(significance_row(&c.id, s))
        })
        .collect()
}

fn significance_row(id: &str, s: &Significance) -> Vec<String> {
    let b = s.bootstrap.as_ref();
    vec![
        id.to_string(),
        s.n_returns.to_string(),
        s.sr_bar.to_string(),
        s.sr_annual.to_string(),
        s.skew.to_string(),
        s.kurtosis.to_string(),
        opt_f(s.lo_se_annual),
        opt_f(s.psr_zero),
        opt_f(s.psr_p_value),
        opt_f(s.min_trl_bars),
        s.min_trl_ok.map(|v| v.to_string()).unwrap_or_default(),
        s.trials.to_string(),
        opt_f(s.trial_sr_variance),
        opt_f(s.expected_max_sr_annual),
        opt_f(s.dsr),
        opt_f(s.bh_q),
        s.bh_group_size.map_or_else(String::new, |v| v.to_string()),
        b.map_or_else(String::new, |x| x.block_len.to_string()),
        b.map_or_else(String::new, |x| x.resamples.to_string()),
        opt_f(b.map(|x| x.mean_sr)),
        opt_f(b.map(|x| x.se_sr)),
        opt_f(b.map(|x| x.ci_low)),
        opt_f(b.map(|x| x.ci_high)),
    ]
}

const DRAWDOWN_HEADER: &[&str] = &[
    "combo_id",
    "rank",
    "peak_time_ms",
    "trough_time_ms",
    "recovery_time_ms",
    "depth",
    "peak_to_trough_bars",
    "recovery_bars",
    "total_bars",
];

fn drawdown_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.drawdowns
        .iter()
        .map(|d| {
            vec![
                d.combo_id.clone(),
                d.rank.to_string(),
                d.peak_time.to_string(),
                d.trough_time.to_string(),
                d.recovery_time.map_or_else(String::new, |v| v.to_string()),
                d.depth.to_string(),
                d.peak_to_trough_bars.to_string(),
                d.recovery_bars.map_or_else(String::new, |v| v.to_string()),
                d.total_bars.to_string(),
            ]
        })
        .collect()
}

const CALENDAR_HEADER: &[&str] = &[
    "combo_id",
    "period",
    "start_time_ms",
    "end_time_ms",
    "return",
    "bars",
    "max_drawdown",
];

fn calendar_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.calendar
        .iter()
        .map(|p| {
            vec![
                p.combo_id.clone(),
                p.period.clone(),
                p.start_time.to_string(),
                p.end_time.to_string(),
                p.return_ratio.to_string(),
                p.bars.to_string(),
                p.max_drawdown.to_string(),
            ]
        })
        .collect()
}

const TRADE_HEADER: &[&str] = &[
    "combo_id",
    "n",
    "long",
    "short",
    "wins",
    "losses",
    "breakeven",
    "win_rate",
    "gross_profit",
    "gross_loss",
    "profit_factor",
    "avg_pnl_pct",
    "median_pnl_pct",
    "avg_fixed_pnl",
    "median_fixed_pnl",
    "best_fixed_pnl",
    "worst_fixed_pnl",
    "avg_bars_held",
    "median_bars_held",
    "avg_mae",
    "avg_mfe",
    "edge_ratio",
    "avg_long_pnl_pct",
    "avg_short_pnl_pct",
    "long_win_rate",
    "short_win_rate",
    "exit_reasons",
];

fn trade_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.trade_stats
        .iter()
        .map(|t| {
            let s = &t.stats;
            vec![
                t.combo_id.clone(),
                s.n.to_string(),
                s.long.to_string(),
                s.short.to_string(),
                s.wins.to_string(),
                s.losses.to_string(),
                s.breakeven.to_string(),
                opt_f(s.win_rate),
                s.gross_profit.to_string(),
                s.gross_loss.to_string(),
                opt_f(s.profit_factor),
                opt_f(s.avg_pnl_pct),
                opt_f(s.median_pnl_pct),
                opt_f(s.avg_fixed_pnl),
                opt_f(s.median_fixed_pnl),
                opt_f(s.best_fixed_pnl),
                opt_f(s.worst_fixed_pnl),
                opt_f(s.avg_bars_held),
                opt_f(s.median_bars_held),
                opt_f(s.avg_mae),
                opt_f(s.avg_mfe),
                opt_f(s.edge_ratio),
                opt_f(s.avg_long_pnl_pct),
                opt_f(s.avg_short_pnl_pct),
                opt_f(s.long_win_rate),
                opt_f(s.short_win_rate),
                s.exit_reasons
                    .iter()
                    .map(|x| format!("{}={}", x.name, x.count))
                    .collect::<Vec<_>>()
                    .join("; "),
            ]
        })
        .collect()
}

const MARGINAL_HEADER: &[&str] = &[
    "group_id",
    "param",
    "value",
    "numeric",
    "n",
    "mean_sharpe",
    "median_sharpe",
    "std_sharpe",
    "mean_return",
];

fn marginal_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.parameter_marginals
        .iter()
        .map(|m| {
            vec![
                m.group_id.clone(),
                m.param.clone(),
                m.value.clone(),
                opt_f(m.numeric),
                m.n.to_string(),
                m.mean_sharpe.to_string(),
                m.median_sharpe.to_string(),
                m.std_sharpe.to_string(),
                m.mean_return.to_string(),
            ]
        })
        .collect()
}

const EXCLUSION_HEADER: &[&str] = &[
    "id",
    "symbol",
    "timeframe",
    "strategy",
    "params",
    "status",
    "reason",
];

fn exclusion_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.exclusions
        .iter()
        .map(|e| {
            vec![
                e.id.clone(),
                e.symbol.clone(),
                e.timeframe.clone(),
                e.strategy.clone(),
                e.params.clone(),
                e.status.clone(),
                e.reason.clone(),
            ]
        })
        .collect()
}

fn status_rows(r: &AnalysisReport) -> Vec<Vec<String>> {
    r.coverage
        .status_counts
        .iter()
        .map(|c| vec![c.name.clone(), c.count.to_string()])
        .collect()
}

// ---------------------------------------------------------------------------
// 字符串工具
// ---------------------------------------------------------------------------

fn csv(rows: &[Vec<String>], header: &[&str]) -> String {
    let mut out = String::new();
    out.push_str(
        &header
            .iter()
            .map(|h| field(h))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in rows {
        out.push_str(&row.iter().map(|v| field(v)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn field(v: &str) -> String {
    if v.contains(',') || v.contains('"') || v.contains('\n') || v.contains('\r') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

fn opt_f(v: Option<f64>) -> String {
    v.map_or_else(String::new, |x| x.to_string())
}

fn fmt_num(v: f64, digits: usize) -> String {
    if v.is_finite() {
        format!("{v:.digits$}")
    } else {
        "-".to_string()
    }
}

fn opt_num(v: Option<f64>, digits: usize) -> String {
    v.map_or_else(|| "-".to_string(), |x| fmt_num(x, digits))
}

fn fmt_pct(v: f64) -> String {
    if v.is_finite() {
        format!("{:.2}%", v * 100.0)
    } else {
        "-".to_string()
    }
}

fn fmt_ts(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map_or_else(|| ms.to_string(), |dt| dt.format("%Y-%m-%d").to_string())
}

fn time_range(r: &AnalysisReport) -> String {
    match (r.input.start_time, r.input.end_time) {
        (Some(a), Some(b)) => format!("{} ~ {}", fmt_ts(a), fmt_ts(b)),
        _ => "无 detail 时间轴".to_string(),
    }
}

fn join_counts(r: &AnalysisReport) -> String {
    r.coverage
        .status_counts
        .iter()
        .map(|c| format!("`{}`={}", c.name, c.count))
        .collect::<Vec<_>>()
        .join(", ")
}

fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    out.push('|');
    for h in header {
        let _ = write!(out, " {} |", md_cell(h));
    }
    out.push('\n');
    out.push('|');
    for _ in header {
        out.push_str("---|");
    }
    out.push('\n');
    for row in rows {
        out.push('|');
        for cell in row {
            let _ = write!(out, " {} |", md_cell(cell));
        }
        out.push('\n');
    }
    out
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn params_of(combo_id: &str) -> String {
    combo_id
        .rsplit_once("|params=")
        .map_or_else(|| combo_id.to_string(), |(_, p)| p.to_string())
}

fn short_group(g: &GroupAnalysis) -> String {
    format!("{}/{}/{}", g.symbol, g.timeframe, g.strategy)
}

fn exclusion_groups(r: &AnalysisReport) -> Vec<(String, usize)> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for e in &r.exclusions {
        for part in e.reason.split("; ") {
            let key = part.split(':').next().unwrap_or(part).to_string();
            *map.entry(key).or_insert(0) += 1;
        }
    }
    let mut v: Vec<(String, usize)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}
