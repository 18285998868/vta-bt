//! 分析主编排: 装载结果 → 对账 → 描述性统计 → 显著性 → 报告模型。

use std::collections::{BTreeMap, HashMap};

use vta_core::engine::{CurveAccum, bar_log_returns};
use vta_core::{RunResult, RunStatus};

use crate::bootstrap::stationary_bootstrap_sr;
use crate::calendar::{CalendarSummary, calendar_summary};
use crate::drawdown::{DrawdownSummary, cvar, drawdown_summary};
use crate::metrics::{
    benjamini_hochberg, dsr, expected_max_sr, lo_standard_error, min_trl, psr, return_moments,
};
use crate::model::{
    AnalysisReport, AnalyzeOptions, ComboAnalysis, Coverage, DrawdownRow, Exclusion, GroupAnalysis,
    InputSummary, ParamMarginalRow, PboSummary, Reconciliation, ReconciliationCheck, ReportBundle,
    Significance, StabilitySummaryPub, TradeStatsRow,
};
use crate::stability::{StabilityRow, stability_summary};
use crate::stats::{canonical_bits, finite, fnv1a64, mean, variance};
use crate::trades::{CountItem, TradeStats, trade_stats};

/// 显著性分析至少需要的收益笔数。
const MIN_RETURNS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WindowKey {
    bars: u32,
    bh_return_bits: u64,
    bh_pnl_bits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    symbol: String,
    timeframe: String,
    strategy: String,
    direction: String,
    fee_bps_bits: u64,
    slippage_bps_bits: u64,
    delay_bars: u32,
    fill_close: bool,
    window: WindowKey,
}

fn window_key(row: &RunResult) -> WindowKey {
    WindowKey {
        bars: row.bars_used,
        bh_return_bits: canonical_bits(row.buy_hold_return),
        bh_pnl_bits: canonical_bits(row.buy_hold_pnl.to_f64()),
    }
}

fn group_key(row: &RunResult) -> GroupKey {
    GroupKey {
        symbol: row.symbol.clone(),
        timeframe: row.timeframe.clone(),
        strategy: row.strategy.clone(),
        direction: row.metrics.direction.as_str().to_string(),
        fee_bps_bits: canonical_bits(row.metrics.fee_bps),
        slippage_bps_bits: canonical_bits(row.metrics.slippage_bps),
        delay_bars: row.metrics.delay_bars,
        fill_close: row.fill_close,
        window: window_key(row),
    }
}

fn group_id(g: &GroupKey) -> String {
    format!(
        "{}|{}|{}|{}|fee={:016x}|slip={:016x}|delay={}|fill={}|bars={}|bh={:016x}{:016x}",
        g.symbol,
        g.timeframe,
        g.strategy,
        g.direction,
        g.fee_bps_bits,
        g.slippage_bps_bits,
        g.delay_bars,
        g.fill_close,
        g.window.bars,
        g.window.bh_return_bits,
        g.window.bh_pnl_bits
    )
}

fn combo_id(g: &GroupKey, params: &str) -> String {
    format!("{}|params={params}", group_id(g))
}

#[derive(Debug, Clone)]
struct JoinedDetail {
    times: Vec<i64>,
    fixed: Vec<f64>,
    equity: Option<Vec<f64>>,
    trades: Option<Vec<vta_report::TradeRecord>>,
}

struct DetailIndex<'a> {
    fixed: HashMap<(String, String, String, String), Vec<&'a vta_report::FixedCurve>>,
    // `combo` 标识一轮运行, 同组多条参数曲线共享它; 必须再带 params 才能唯一定位。
    equity: HashMap<(String, String), &'a vta_report::EquityCurve>,
    trades: HashMap<(String, String), &'a vta_report::TradeSet>,
}

fn build_index(detail: Option<&vta_report::DetailBundle>) -> DetailIndex<'_> {
    let mut fixed: HashMap<(String, String, String, String), Vec<&vta_report::FixedCurve>> =
        HashMap::new();
    let mut equity = HashMap::new();
    let mut trades = HashMap::new();
    if let Some(d) = detail {
        for c in &d.fixed {
            fixed
                .entry((
                    c.symbol.clone(),
                    c.timeframe.clone(),
                    c.strategy.clone(),
                    c.params.clone(),
                ))
                .or_default()
                .push(c);
        }
        for e in &d.equity {
            equity.insert((e.combo.clone(), e.params.clone()), e);
        }
        for t in &d.trades {
            trades.insert((t.combo.clone(), t.params.clone()), t);
        }
    }
    DetailIndex {
        fixed,
        equity,
        trades,
    }
}

fn join_detail(
    index: &DetailIndex<'_>,
    row: &RunResult,
) -> std::result::Result<Option<JoinedDetail>, String> {
    let key = (
        row.symbol.clone(),
        row.timeframe.clone(),
        row.strategy.clone(),
        row.params.clone(),
    );
    let Some(candidates) = index.fixed.get(&key) else {
        return Ok(None);
    };
    if candidates.len() > 1 {
        let first = candidates[0];
        let same = candidates
            .iter()
            .all(|c| c.times == first.times && c.values == first.values);
        if !same {
            return Err(format!(
                "detail 中同一 (symbol,timeframe,strategy,params) 有 {} 条不同曲线; 无法确定使用哪一条",
                candidates.len()
            ));
        }
    }
    let fixed = candidates[0];
    if fixed.status != row.status {
        return Err(format!(
            "detail status={} 与 result.parquet status={} 不一致",
            fixed.status.as_str(),
            row.status.as_str()
        ));
    }
    let equity = index
        .equity
        .get(&(fixed.combo.clone(), fixed.params.clone()))
        .map(|e| e.values.clone());
    let trades = index
        .trades
        .get(&(fixed.combo.clone(), fixed.params.clone()))
        .map(|t| t.trades.clone());
    Ok(Some(JoinedDetail {
        times: fixed.times.clone(),
        fixed: fixed.values.clone(),
        equity,
        trades,
    }))
}

fn check(
    checks: &mut Vec<ReconciliationCheck>,
    errors: &mut Vec<String>,
    name: &str,
    engine: f64,
    recomputed: f64,
    tolerance: f64,
) {
    if !engine.is_finite() || !recomputed.is_finite() {
        errors.push(format!(
            "{name}: engine={engine} recomputed={recomputed} 非有限值"
        ));
        checks.push(ReconciliationCheck {
            name: name.to_string(),
            engine: 0.0,
            recomputed: 0.0,
            abs_diff: f64::INFINITY,
            tolerance,
            ok: false,
        });
        return;
    }
    let abs_diff = (engine - recomputed).abs();
    checks.push(ReconciliationCheck {
        name: name.to_string(),
        engine,
        recomputed,
        abs_diff,
        tolerance,
        ok: abs_diff <= tolerance,
    });
}

fn curve_stats(values: &[f64], bpy: f64) -> Option<vta_core::engine::CurveStats> {
    if values.len() < 2 {
        return None;
    }
    let mut acc = CurveAccum::new(values[0]);
    for &v in values {
        acc.push(v);
    }
    Some(acc.stats(bpy))
}

fn check_curve(
    checks: &mut Vec<ReconciliationCheck>,
    errors: &mut Vec<String>,
    label: &str,
    stats: Option<vta_core::engine::CurveStats>,
    engine: (f64, f64, f64),
    tol: f64,
) {
    let Some(stats) = stats else {
        errors.push(format!("{label}: 曲线不足 2 点, 无法复算"));
        return;
    };
    check(
        checks,
        errors,
        &format!("{label}.sharpe"),
        engine.0,
        stats.sharpe,
        tol,
    );
    check(
        checks,
        errors,
        &format!("{label}.max_drawdown"),
        engine.1,
        stats.max_dd,
        tol,
    );
    check(
        checks,
        errors,
        &format!("{label}.total_return"),
        engine.2,
        stats.total_return,
        tol,
    );
}

fn reconcile(row: &RunResult, detail: &JoinedDetail, bpy: f64) -> Reconciliation {
    let mut checks = Vec::new();
    let mut errors = Vec::new();

    if detail.times.len() != detail.fixed.len() {
        errors.push(format!(
            "time 列 {} != fixed 列 {}",
            detail.times.len(),
            detail.fixed.len()
        ));
    }
    if detail.times.windows(2).any(|w| w[0] >= w[1]) {
        errors.push("time 列非严格递增".to_string());
    }
    if detail.fixed.iter().any(|v| !v.is_finite()) {
        errors.push("fixed 曲线含非有限值".to_string());
    }
    if let Some(eq) = &detail.equity {
        if eq.len() != detail.fixed.len() {
            errors.push(format!(
                "equity 曲线 {} 行 != fixed 曲线 {} 行",
                eq.len(),
                detail.fixed.len()
            ));
        }
        if eq.iter().any(|v| !v.is_finite()) {
            errors.push("equity 曲线含非有限值".to_string());
        }
    }
    let bars_engine = f64::from(row.bars_used);
    check(
        &mut checks,
        &mut errors,
        "bars_used",
        bars_engine,
        detail.fixed.len() as f64,
        0.0,
    );

    let fixed_stats = curve_stats(&detail.fixed, bpy);
    check_curve(
        &mut checks,
        &mut errors,
        "fixed",
        fixed_stats,
        (
            row.metrics.fixed_sharpe_ratio,
            row.metrics.fixed_max_drawdown_pct / 100.0,
            row.metrics.fixed_total_return,
        ),
        1e-8,
    );
    if let Some(eq) = &detail.equity {
        let comp_stats = curve_stats(eq, bpy);
        check_curve(
            &mut checks,
            &mut errors,
            "compound",
            comp_stats,
            (
                row.metrics.sharpe_ratio,
                row.metrics.max_drawdown_pct / 100.0,
                row.metrics.total_return,
            ),
            1e-8,
        );
    }
    if let Some(trades) = &detail.trades {
        check(
            &mut checks,
            &mut errors,
            "trades.count",
            f64::from(row.metrics.num_trades),
            trades.len() as f64,
            0.0,
        );
        let sum_fixed: f64 = trades.iter().map(|t| t.fixed_pnl.to_f64()).sum();
        check(
            &mut checks,
            &mut errors,
            "trades.sum_fixed_pnl",
            row.metrics.total_fixed_pnl.to_f64(),
            sum_fixed,
            0.01,
        );
        if let Some(eq) = &detail.equity {
            let sum_pnl: f64 = trades.iter().map(|t| t.pnl.to_f64()).sum();
            let initial = eq.first().copied().unwrap_or(0.0);
            check(
                &mut checks,
                &mut errors,
                "trades.sum_pnl_plus_initial",
                row.metrics.final_equity.to_f64(),
                initial + sum_pnl,
                0.01,
            );
        }
        let count_reason =
            |name: &str| trades.iter().filter(|t| t.exit_reason == name).count() as f64;
        check(
            &mut checks,
            &mut errors,
            "trades.stop_loss_exits",
            f64::from(row.metrics.stop_loss_exits),
            count_reason("stop_loss"),
            0.0,
        );
        check(
            &mut checks,
            &mut errors,
            "trades.trailing_stop_exits",
            f64::from(row.metrics.trailing_stop_exits),
            count_reason("trailing_stop"),
            0.0,
        );
        check(
            &mut checks,
            &mut errors,
            "trades.strategy_stop_exits",
            f64::from(row.metrics.strategy_stop_exits),
            count_reason("strategy_stop"),
            0.0,
        );
        let mut wins = 0usize;
        let mut losses = 0usize;
        let mut gross_win = 0.0;
        let mut gross_loss = 0.0;
        for t in trades {
            let p = t.pnl.to_f64();
            if p > 0.0 {
                wins += 1;
                gross_win += p;
            } else if p < 0.0 {
                losses += 1;
                gross_loss += -p;
            }
        }
        let decided = wins + losses;
        let win_rate = if decided > 0 {
            wins as f64 / decided as f64 * 100.0
        } else {
            0.0
        };
        check(
            &mut checks,
            &mut errors,
            "trades.win_rate",
            row.metrics.win_rate,
            win_rate,
            1e-8,
        );
        let pf = if gross_loss > 0.0 {
            gross_win / gross_loss
        } else {
            gross_win.max(0.0)
        };
        check(
            &mut checks,
            &mut errors,
            "trades.profit_factor",
            row.metrics.profit_factor,
            pf,
            1e-8,
        );
        let avg_hold = if trades.is_empty() {
            0.0
        } else {
            trades.iter().map(|t| t.bars_held as f64).sum::<f64>() / trades.len() as f64
        };
        check(
            &mut checks,
            &mut errors,
            "trades.avg_hold_bars",
            row.metrics.avg_hold_bars,
            avg_hold,
            1e-8,
        );
    }

    Reconciliation {
        detail_available: true,
        checked: true,
        consistent: errors.is_empty() && checks.iter().all(|c| c.ok),
        checks,
        errors,
    }
}

#[derive(Debug, Clone, Copy)]
struct CurveExtras {
    volatility: Option<f64>,
    ulcer: Option<f64>,
    cvar_95: Option<f64>,
    worst_bar: Option<f64>,
}

fn extras(values: &[f64], times: &[i64], bpy: f64) -> CurveExtras {
    let rets: Vec<f64> = bar_log_returns(values).collect();
    let stats = curve_stats(values, bpy);
    let dd = drawdown_summary(times, values, 5);
    CurveExtras {
        volatility: stats.map(|s| s.volatility / 100.0),
        ulcer: dd.map(|d| d.ulcer_index),
        cvar_95: cvar(&rets, 0.05),
        worst_bar: rets.iter().copied().min_by(f64::total_cmp),
    }
}

#[derive(Debug, Clone)]
struct ComboRecord {
    id: String,
    group: GroupKey,
    row: RunResult,
    detail: Option<JoinedDetail>,
    reconciliation: Reconciliation,
    rank_eligible: bool,
    significance_eligible: bool,
    significance_note: Option<String>,
    exclusion: Vec<String>,
    bpy: f64,
    trial_sr_variance: Option<f64>,
    trials: usize,
    fixed_extra: Option<CurveExtras>,
    compound_extra: Option<CurveExtras>,
    drawdown: Option<DrawdownSummary>,
    calendar: Option<CalendarSummary>,
    trade_stats: Option<TradeStats>,
    significance: Option<Significance>,
}

fn build_record(row: &RunResult, index: &DetailIndex<'_>, opts: &AnalyzeOptions) -> ComboRecord {
    let group = group_key(row);
    let id = combo_id(&group, &row.params);
    let bpy = vta_core::timeframe::bars_per_year(&row.timeframe);
    let mut exclusion = Vec::new();
    let (detail, detail_error) = match join_detail(index, row) {
        Ok(d) => (d, None),
        Err(e) => (None, Some(e)),
    };
    if row.status != RunStatus::Ok {
        exclusion.push(format!("status:{}", row.status.as_str()));
    }
    if !vta_core::timeframe::is_known_timeframe(&row.timeframe) {
        exclusion.push(format!("unknown_timeframe:{}", row.timeframe));
    }
    if row.metrics.num_trades < opts.min_trades {
        exclusion.push(format!(
            "trades_below_min:{}/{}",
            row.metrics.num_trades, opts.min_trades
        ));
    }
    if let Some(e) = &detail_error {
        exclusion.push(format!("detail_join:{e}"));
    }
    let reconciliation = match &detail {
        Some(d) => reconcile(row, d, bpy),
        None => Reconciliation {
            detail_available: false,
            checked: false,
            consistent: true,
            checks: Vec::new(),
            errors: Vec::new(),
        },
    };
    if reconciliation.checked && !reconciliation.consistent {
        exclusion.push("reconciliation_failed".to_string());
    }
    let rank_eligible = exclusion.is_empty();
    let significance_note = match &detail {
        None => Some("no_detail".to_string()),
        Some(d) => {
            let n = d.fixed.len().saturating_sub(1);
            if n < MIN_RETURNS {
                Some(format!("returns_too_short:{n}<{MIN_RETURNS}"))
            } else {
                None
            }
        }
    };
    let mut significance_eligible =
        rank_eligible && detail.is_some() && significance_note.is_none();

    let fixed_extra = detail.as_ref().map(|d| extras(&d.fixed, &d.times, bpy));
    let compound_extra = detail
        .as_ref()
        .and_then(|d| d.equity.as_ref().map(|eq| extras(eq, &d.times, bpy)));
    let drawdown = detail
        .as_ref()
        .and_then(|d| drawdown_summary(&d.times, &d.fixed, 10));
    let calendar = detail
        .as_ref()
        .map(|d| calendar_summary(&d.times, &d.fixed));
    let trade_stats = detail
        .as_ref()
        .and_then(|d| d.trades.as_ref().map(|t| trade_stats(t)));

    if significance_eligible {
        if let Some(d) = detail.as_ref() {
            let rets: Vec<f64> = bar_log_returns(&d.fixed).collect();
            if return_moments(&rets).is_none() {
                significance_eligible = false;
            }
        } else {
            significance_eligible = false;
        }
    }

    ComboRecord {
        id,
        group,
        row: row.clone(),
        detail,
        reconciliation,
        rank_eligible,
        significance_eligible,
        significance_note,
        exclusion,
        bpy,
        trial_sr_variance: None,
        trials: 1,
        fixed_extra,
        compound_extra,
        drawdown,
        calendar,
        trade_stats,
        significance: None,
    }
}

fn compute_significance(
    record: &ComboRecord,
    trial_sr_variance: Option<f64>,
    trials: usize,
    alpha: f64,
) -> Option<Significance> {
    let detail = record.detail.as_ref()?;
    let rets: Vec<f64> = bar_log_returns(&detail.fixed).collect();
    let m = return_moments(&rets)?;
    let p = psr(&m, 0.0);
    let d = dsr(&m, trial_sr_variance.unwrap_or(0.0), trials);
    let lo = lo_standard_error(&m).map(|x| x * record.bpy.sqrt());
    let min_t = min_trl(&m, 0.0, alpha).map(|t| t + 1.0);
    let expected_annual =
        expected_max_sr(trial_sr_variance.unwrap_or(0.0), trials).map(|x| x * record.bpy.sqrt());
    Some(Significance {
        n_returns: m.n,
        sr_bar: m.sr_bar,
        sr_annual: m.sr_bar * record.bpy.sqrt(),
        skew: m.skew,
        kurtosis: m.kurtosis,
        lo_se_annual: finite(lo.unwrap_or(f64::NAN)),
        psr_zero: finite(p.unwrap_or(f64::NAN)),
        psr_p_value: p.map(|x| (1.0 - x).clamp(0.0, 1.0)),
        min_trl_bars: finite(min_t.unwrap_or(f64::NAN)),
        min_trl_ok: min_t.map(|t| t <= detail.fixed.len() as f64),
        dsr: finite(d.unwrap_or(f64::NAN)),
        trials,
        trial_sr_variance: finite(trial_sr_variance.unwrap_or(f64::NAN)),
        expected_max_sr_annual: finite(expected_annual.unwrap_or(f64::NAN)),
        bootstrap: None,
        bh_q: None,
        bh_group_size: None,
    })
}

/// PBO 输入: (参数串, 时间轴, fixed 曲线)。
type PboCurve<'a> = (String, &'a [i64], &'a [f64]);

fn pbo_for_group(
    records: &[ComboRecord],
    indices: &[usize],
    bpy: f64,
    warnings: &mut Vec<String>,
) -> Option<PboSummary> {
    let mut curves: Vec<PboCurve<'_>> = Vec::new();
    for &i in indices {
        let r = &records[i];
        if !r.rank_eligible {
            continue;
        }
        let Some(d) = &r.detail else {
            continue;
        };
        curves.push((r.row.params.clone(), &d.times, &d.fixed));
    }
    if curves.len() < 10 {
        return None;
    }
    let mut cohorts: Vec<Vec<PboCurve<'_>>> = Vec::new();
    'curve: for c in curves {
        for cohort in &mut cohorts {
            if cohort[0].1 == c.1 {
                cohort.push(c);
                continue 'curve;
            }
        }
        cohorts.push(vec![c]);
    }
    let mut cohort = cohorts
        .into_iter()
        .max_by_key(|c| (c.len(), c[0].1.len()))?;
    cohort.sort_by(|a, b| a.0.cmp(&b.0));
    cohort.dedup_by(|a, b| a.0 == b.0);
    if cohort.len() < 10 {
        warnings.push(format!(
            "PBO 跳过: 同时间网格的曲线只有 {} 条(<10)",
            cohort.len()
        ));
        return None;
    }
    let t = cohort[0].1.len();
    let Some(s) = vta_pbo::pick_s(t) else {
        warnings.push(format!("PBO 跳过: T={t} < 32"));
        return None;
    };
    let data: Vec<Vec<f64>> = cohort.iter().map(|c| c.2.to_vec()).collect();
    match vta_pbo::cscv(&data, s, bpy) {
        Ok(res) => Some(PboSummary {
            n_strategies: res.n_strategies,
            t_bars: res.t_bars,
            s_blocks: res.s_blocks,
            pbo: res.pbo,
            full_sample_best_params: cohort
                .get(res.full_sample_best_idx)
                .map_or_else(String::new, |c| c.0.clone()),
            full_sample_best_sharpe: res.full_sample_best_sharpe,
            degradation: res.degradation,
        }),
        Err(e) => {
            warnings.push(format!("PBO 计算失败: {e}"));
            None
        }
    }
}

fn status_counts(rows: &[RunResult]) -> Vec<CountItem> {
    let mut map: BTreeMap<&'static str, usize> = BTreeMap::new();
    for row in rows {
        *map.entry(row.status.as_str()).or_default() += 1;
    }
    let mut out: Vec<CountItem> = map
        .into_iter()
        .map(|(name, count)| CountItem {
            name: name.to_string(),
            count,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out
}

fn unique_sorted(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = values.collect();
    v.sort();
    v.dedup();
    v
}

/// 对已装载的报告执行分析。纯函数, 便于用合成曲线做单测。
///
/// # Errors
/// 参数非法、行数超过 `max_curves`, 或统计计算发现无法序列化的非有限值时
/// 返回 `AppError`(vta_core::error::AppError)。
pub fn analyze_bundle(
    bundle: &ReportBundle,
    opts: &AnalyzeOptions,
) -> vta_core::error::Result<AnalysisReport> {
    validate_options(opts)?;
    if let Some(d) = &bundle.detail
        && d.fixed.len() > opts.max_curves
    {
        return Err(vta_core::error::AppError::Msg(format!(
            "detail 曲线数 {} 超过 --max-curves {}; 请缩小报告或提高上限",
            d.fixed.len(),
            opts.max_curves
        )));
    }
    let index = build_index(bundle.detail.as_ref());
    let mut warnings = bundle
        .detail
        .as_ref()
        .map_or_else(Vec::new, |d| d.warnings.clone());
    let mut records: Vec<ComboRecord> = bundle
        .rows
        .iter()
        .map(|r| build_record(r, &index, opts))
        .collect();

    let mut groups: BTreeMap<GroupKey, Vec<usize>> = BTreeMap::new();
    for (i, r) in records.iter().enumerate() {
        groups.entry(r.group.clone()).or_default().push(i);
    }

    // 组内试验的逐 bar 夏普方差与 DSR。
    // 试验数是"实际跑过的网格行数", 不是"通过排名门槛的行数"; 否则失败或
    // 样本不足的试验会被从选择偏差里抹掉, DSR 反而被高估。未知周期无法
    // 换算逐 bar 夏普, 仍按硬排除处理。
    for indices in groups.values() {
        let srs: Vec<f64> = indices
            .iter()
            .filter(|&&i| records[i].row.status == RunStatus::Ok)
            .filter(|&&i| vta_core::timeframe::is_known_timeframe(&records[i].row.timeframe))
            .map(|&i| {
                let bpy = records[i].bpy.sqrt();
                records[i].row.metrics.fixed_sharpe_ratio / bpy
            })
            .filter(|v| v.is_finite())
            .collect();
        let var = if srs.len() >= 2 {
            let m = mean(&srs);
            variance(&srs, m)
        } else {
            None
        };
        let trials = opts.trials.unwrap_or(srs.len().max(1));
        for &i in indices {
            records[i].trial_sr_variance = var;
            records[i].trials = trials;
        }
    }

    // 单组合显著性。
    for r in &mut records {
        if !r.significance_eligible {
            continue;
        }
        let sig = compute_significance(r, r.trial_sr_variance, r.trials, opts.alpha);
        if sig.is_none() {
            r.significance_eligible = false;
            r.significance_note = Some("moments_unavailable".to_string());
        }
        r.significance = sig;
    }

    // BH 只在同组内做。
    for indices in groups.values() {
        let mut with_p: Vec<(usize, f64)> = indices
            .iter()
            .filter_map(|&i| {
                records[i]
                    .significance
                    .as_ref()
                    .and_then(|s| s.psr_p_value)
                    .map(|p| (i, p))
            })
            .collect();
        with_p.sort_by(|(a, pa), (b, pb)| {
            pa.total_cmp(pb)
                .then_with(|| records[*a].id.cmp(&records[*b].id))
        });
        let ps: Vec<f64> = with_p.iter().map(|(_, p)| *p).collect();
        let qs = benjamini_hochberg(&ps);
        let m = with_p.len();
        for (k, &(i, _)) in with_p.iter().enumerate() {
            if let Some(s) = records[i].significance.as_mut() {
                s.bh_q = finite(qs[k]);
                s.bh_group_size = Some(m);
            }
        }
        let group_rows = indices.len();
        if m > 0 && m < group_rows {
            warnings.push(format!(
                "BH 仅覆盖 {m}/{group_rows} 行(其余缺 detail 或曲线过短); q 值偏乐观"
            ));
        }
    }

    // 排名。
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = &records[a];
        let rb = &records[b];
        rb.rank_eligible
            .cmp(&ra.rank_eligible)
            .then_with(|| {
                rb.row
                    .metrics
                    .fixed_sharpe_ratio
                    .total_cmp(&ra.row.metrics.fixed_sharpe_ratio)
            })
            .then_with(|| {
                rb.row
                    .metrics
                    .fixed_total_return
                    .total_cmp(&ra.row.metrics.fixed_total_return)
            })
            .then_with(|| ra.id.cmp(&rb.id))
    });
    let top_ids: Vec<String> = order
        .iter()
        .filter(|&&i| records[i].rank_eligible)
        .take(opts.top)
        .map(|&i| records[i].id.clone())
        .collect();
    // 表格类产物只覆盖"有 detail 的 Top-N": 缺 detail 的组合无法给出曲线级
    // 回撤/日历, 留空位会让整张表消失。
    let detail_top_ids: Vec<String> = order
        .iter()
        .filter(|&&i| records[i].rank_eligible && records[i].detail.is_some())
        .take(opts.top)
        .map(|&i| records[i].id.clone())
        .collect();

    // Top-N 的 bootstrap 置信区间。
    if opts.bootstrap > 0 {
        for id in &detail_top_ids {
            let Some(i) = records.iter().position(|r| &r.id == id) else {
                continue;
            };
            let Some(detail) = &records[i].detail else {
                continue;
            };
            let rets: Vec<f64> = bar_log_returns(&detail.fixed).collect();
            let seed = opts.seed ^ fnv1a64(id.as_bytes());
            let boot = stationary_bootstrap_sr(
                &rets,
                records[i].bpy,
                opts.bootstrap,
                opts.block_len,
                seed,
                opts.alpha,
            );
            if let (Some(b), Some(sig)) = (boot, records[i].significance.as_mut()) {
                sig.bootstrap = Some(b);
            }
        }
    }

    // 输出组合。
    let combos: Vec<ComboAnalysis> = order
        .iter()
        .map(|&i| {
            let r = &records[i];
            ComboAnalysis {
                id: r.id.clone(),
                group_id: group_id(&r.group),
                symbol: r.row.symbol.clone(),
                timeframe: r.row.timeframe.clone(),
                strategy: r.row.strategy.clone(),
                params: r.row.params.clone(),
                direction: r.group.direction.clone(),
                fee_bps: r.row.metrics.fee_bps,
                slippage_bps: r.row.metrics.slippage_bps,
                delay_bars: r.row.metrics.delay_bars,
                fill_close: r.row.fill_close,
                window_bars: r.row.bars_used,
                status: r.row.status.as_str().to_string(),
                trades: r.row.metrics.num_trades,
                bars_used: r.row.bars_used,
                fixed_return: r.row.metrics.fixed_total_return,
                fixed_sharpe: r.row.metrics.fixed_sharpe_ratio,
                fixed_max_drawdown: r.row.metrics.fixed_max_drawdown_pct / 100.0,
                fixed_volatility: r.fixed_extra.and_then(|e| e.volatility),
                fixed_ulcer: r.fixed_extra.and_then(|e| e.ulcer),
                fixed_cvar_95: r.fixed_extra.and_then(|e| e.cvar_95),
                fixed_worst_bar: r.fixed_extra.and_then(|e| e.worst_bar),
                compound_return: r.row.metrics.total_return,
                compound_sharpe: r.row.metrics.sharpe_ratio,
                compound_max_drawdown: r.row.metrics.max_drawdown_pct / 100.0,
                compound_ulcer: r.compound_extra.and_then(|e| e.ulcer),
                compound_cvar_95: r.compound_extra.and_then(|e| e.cvar_95),
                rank_eligible: r.rank_eligible,
                significance_eligible: r.significance_eligible,
                significance_note: r.significance_note.clone(),
                exclusion: r.exclusion.clone(),
                reconciliation: r.reconciliation.clone(),
                significance: r.significance.clone(),
            }
        })
        .collect();

    // Top-N 的回撤与日历; 只取有 detail 的组合, 名次保留全量排名。
    let mut drawdowns = Vec::new();
    let mut calendar = Vec::new();
    for id in &detail_top_ids {
        let Some(i) = records.iter().position(|r| &r.id == id) else {
            continue;
        };
        let rank = order
            .iter()
            .position(|&j| &records[j].id == id)
            .map_or(0, |p| p + 1);
        let r = &records[i];
        if let Some(dd) = &r.drawdown {
            for ep in &dd.episodes {
                drawdowns.push(DrawdownRow {
                    combo_id: id.clone(),
                    rank,
                    peak_time: ep.peak_time,
                    trough_time: ep.trough_time,
                    recovery_time: ep.recovery_time,
                    depth: ep.depth,
                    peak_to_trough_bars: ep.peak_to_trough_bars,
                    recovery_bars: ep.recovery_bars,
                    total_bars: ep.total_bars,
                });
            }
        }
        if let Some(cal) = &r.calendar {
            for p in cal.monthly.iter().chain(cal.yearly.iter()) {
                calendar.push(crate::model::CalendarRow {
                    combo_id: id.clone(),
                    period: p.period.clone(),
                    start_time: p.start_time,
                    end_time: p.end_time,
                    return_ratio: p.return_ratio,
                    bars: p.bars,
                    max_drawdown: p.max_drawdown,
                });
            }
        }
    }

    // 交易结构表(所有有 detail 的组合)。
    let trade_stats: Vec<TradeStatsRow> = records
        .iter()
        .filter_map(|r| {
            r.trade_stats.as_ref().map(|s| TradeStatsRow {
                combo_id: r.id.clone(),
                stats: s.clone(),
            })
        })
        .collect();

    // 分组统计。
    let mut group_out = Vec::with_capacity(groups.len());
    let mut marginals = Vec::new();
    for (key, indices) in &groups {
        let gid = group_id(key);
        let ok_rows = indices
            .iter()
            .filter(|&&i| records[i].row.status == RunStatus::Ok)
            .count();
        let eligible: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| records[i].rank_eligible)
            .collect();
        let sharpes: Vec<f64> = eligible
            .iter()
            .map(|&i| records[i].row.metrics.fixed_sharpe_ratio)
            .collect();
        let best = eligible.iter().copied().max_by(|&a, &b| {
            records[a]
                .row
                .metrics
                .fixed_sharpe_ratio
                .total_cmp(&records[b].row.metrics.fixed_sharpe_ratio)
                .then_with(|| records[b].id.cmp(&records[a].id))
        });
        let stab = stability_summary(
            &eligible
                .iter()
                .map(|&i| StabilityRow {
                    params_json: records[i].row.strategy_params_json.clone(),
                    params: records[i].row.params.clone(),
                    fixed_sharpe: records[i].row.metrics.fixed_sharpe_ratio,
                    fixed_return: records[i].row.metrics.fixed_total_return,
                })
                .collect::<Vec<_>>(),
        );
        for m in &stab.marginals {
            marginals.push(ParamMarginalRow {
                group_id: gid.clone(),
                param: m.param.clone(),
                value: m.value.clone(),
                numeric: finite(m.numeric.unwrap_or(f64::NAN)),
                n: m.n,
                mean_sharpe: m.mean_sharpe,
                median_sharpe: m.median_sharpe,
                std_sharpe: m.std_sharpe,
                mean_return: m.mean_return,
            });
        }
        let pbo = pbo_for_group(&records, indices, records[indices[0]].bpy, &mut warnings);
        group_out.push(GroupAnalysis {
            id: gid,
            symbol: key.symbol.clone(),
            timeframe: key.timeframe.clone(),
            strategy: key.strategy.clone(),
            direction: key.direction.clone(),
            fee_bps: f64::from_bits(key.fee_bps_bits),
            slippage_bps: f64::from_bits(key.slippage_bps_bits),
            delay_bars: key.delay_bars,
            fill_close: key.fill_close,
            window_bars: key.window.bars,
            rows: indices.len(),
            ok_rows,
            rank_eligible: eligible.len(),
            significance_eligible: indices
                .iter()
                .filter(|&&i| records[i].significance_eligible)
                .count(),
            best_params: best.map(|i| records[i].row.params.clone()),
            best_fixed_sharpe: best.map(|i| records[i].row.metrics.fixed_sharpe_ratio),
            median_fixed_sharpe: crate::stats::median(&sharpes),
            share_positive: if sharpes.is_empty() {
                None
            } else {
                Some(sharpes.iter().filter(|s| **s > 0.0).count() as f64 / sharpes.len() as f64)
            },
            stability: StabilitySummaryPub {
                best_params: stab.best_params,
                best_sharpe: stab.best_sharpe,
                median_sharpe: stab.median_sharpe,
                share_within_tolerance: stab.share_within_tolerance,
                neighbor_best_sharpe: stab.neighbor_best_sharpe,
                isolated_peak: stab.isolated_peak,
            },
            pbo,
        });
    }
    group_out.sort_by(|a, b| a.id.cmp(&b.id));
    marginals.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then_with(|| a.param.cmp(&b.param))
            .then_with(|| match (a.numeric, b.numeric) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                _ => a.value.cmp(&b.value),
            })
    });

    // 排除清单。
    let exclusions: Vec<Exclusion> = records
        .iter()
        .filter(|r| !r.exclusion.is_empty())
        .map(|r| Exclusion {
            id: r.id.clone(),
            symbol: r.row.symbol.clone(),
            timeframe: r.row.timeframe.clone(),
            strategy: r.row.strategy.clone(),
            params: r.row.params.clone(),
            status: r.row.status.as_str().to_string(),
            reason: r.exclusion.join("; "),
        })
        .collect();

    let total_rows = records.len();
    let ok_rows = records
        .iter()
        .filter(|r| r.row.status == RunStatus::Ok)
        .count();
    let rank_eligible = records.iter().filter(|r| r.rank_eligible).count();
    let significance_eligible = records.iter().filter(|r| r.significance_eligible).count();
    let inconsistent = records
        .iter()
        .filter(|r| r.reconciliation.checked && !r.reconciliation.consistent)
        .count();
    let detail_missing = records
        .iter()
        .filter(|r| r.row.status == RunStatus::Ok && r.detail.is_none())
        .count();

    let times: Vec<i64> = records
        .iter()
        .filter_map(|r| r.detail.as_ref())
        .flat_map(|d| d.times.iter().copied())
        .collect();
    let start_time = times.iter().copied().min();
    let end_time = times.iter().copied().max();
    let detail_warnings = bundle.detail.as_ref().map_or(0, |d| d.warnings.len());

    Ok(AnalysisReport {
        schema_version: crate::model::SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source_hash: vta_core::source_hash().to_string(),
        input_digest: bundle.input_digest.clone(),
        options: opts.clone(),
        input: InputSummary {
            result_rows: bundle.rows.len(),
            detail_fixed: bundle.detail.as_ref().map_or(0, |d| d.fixed.len()),
            detail_equity: bundle.detail.as_ref().map_or(0, |d| d.equity.len()),
            detail_trades: bundle.detail.as_ref().map_or(0, |d| d.trades.len()),
            detail_manifest_present: bundle.detail.is_some(),
            detail_warnings,
            symbols: unique_sorted(bundle.rows.iter().map(|r| r.symbol.clone())),
            timeframes: unique_sorted(bundle.rows.iter().map(|r| r.timeframe.clone())),
            strategies: unique_sorted(bundle.rows.iter().map(|r| r.strategy.clone())),
            start_time,
            end_time,
        },
        coverage: Coverage {
            total_rows,
            ok_rows,
            rank_eligible,
            significance_eligible,
            inconsistent,
            detail_missing,
            groups: groups.len(),
            status_counts: status_counts(&bundle.rows),
        },
        groups: group_out,
        combos,
        top_ids,
        detail_top_ids,
        drawdowns,
        calendar,
        trade_stats,
        parameter_marginals: marginals,
        exclusions,
        warnings,
    })
}

fn validate_options(opts: &AnalyzeOptions) -> vta_core::error::Result<()> {
    if !(0.0 < opts.alpha && opts.alpha < 1.0) {
        return Err(vta_core::error::AppError::Msg(format!(
            "--alpha 必须在 (0,1): {}",
            opts.alpha
        )));
    }
    if opts.top == 0 {
        return Err(vta_core::error::AppError::Msg(
            "--top 必须 >= 1".to_string(),
        ));
    }
    if opts.max_curves == 0 {
        return Err(vta_core::error::AppError::Msg(
            "--max-curves 必须 >= 1".to_string(),
        ));
    }
    if let Some(0) = opts.block_len {
        return Err(vta_core::error::AppError::Msg(
            "--block-len 必须 >= 1".to_string(),
        ));
    }
    if let Some(0) = opts.trials {
        return Err(vta_core::error::AppError::Msg(
            "--trials 必须 >= 1".to_string(),
        ));
    }
    Ok(())
}

/// 只读分析一个报告目录。
///
/// # Errors
/// 报告缺失/损坏、detail 不完整、参数非法或统计计算失败时返回
/// `AppError`(vta_core::error::AppError)。
pub fn analyze(
    dir: &std::path::Path,
    opts: &AnalyzeOptions,
) -> vta_core::error::Result<AnalysisReport> {
    let bundle = crate::input::load(dir, opts.require_detail)?;
    analyze_bundle(&bundle, opts)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use vta_core::engine::{CurveStats, SummaryMetrics};
    use vta_core::money::Amount;
    use vta_report::{DetailBundle, FixedCurve};

    const BPY: f64 = 365.0;
    const N: usize = 200;

    fn curve(slope: f64) -> Vec<f64> {
        (0..N)
            .map(|i| 100.0 * (1.0 + slope).powi(i as i32))
            .collect()
    }

    fn stats(values: &[f64]) -> CurveStats {
        let mut acc = CurveAccum::new(values[0]);
        for &v in values {
            acc.push(v);
        }
        acc.stats(BPY)
    }

    fn run_row(params: &str, values: &[f64]) -> RunResult {
        let s = stats(values);
        let metrics = SummaryMetrics {
            num_trades: 0,
            fixed_sharpe_ratio: s.sharpe,
            fixed_max_drawdown_pct: s.max_dd * 100.0,
            fixed_total_return: s.total_return,
            ..SummaryMetrics::default()
        };
        RunResult {
            symbol: "X".to_string(),
            timeframe: "1d".to_string(),
            strategy: "s".to_string(),
            params: params.to_string(),
            metrics,
            strategy_params: 0,
            strategy_params_json: "{}".to_string(),
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: values.len() as u32,
            status: RunStatus::Ok,
        }
    }

    fn fixed_curve(params: &str, values: &[f64]) -> FixedCurve {
        FixedCurve {
            combo: format!("combo-{params}"),
            symbol: "X".to_string(),
            timeframe: "1d".to_string(),
            strategy: "s".to_string(),
            params: params.to_string(),
            status: RunStatus::Ok,
            times: (0..values.len() as i64).map(|i| i * 86_400_000).collect(),
            values: values.to_vec(),
        }
    }

    fn bundle() -> ReportBundle {
        let a = curve(0.0020);
        let b = curve(0.0015);
        let c = curve(0.0010);
        let d = curve(0.0005);
        ReportBundle {
            rows: vec![
                run_row("a=1", &a),
                run_row("b=1", &b),
                run_row("c=1", &c),
                run_row("d=1", &d),
            ],
            detail: Some(DetailBundle {
                fixed: vec![
                    fixed_curve("b=1", &b),
                    fixed_curve("c=1", &c),
                    fixed_curve("d=1", &d),
                ],
                equity: Vec::new(),
                trades: Vec::new(),
                warnings: Vec::new(),
            }),
            input_digest: "test".to_string(),
        }
    }

    fn options() -> AnalyzeOptions {
        AnalyzeOptions {
            min_trades: 0,
            bootstrap: 0,
            top: 10,
            ..AnalyzeOptions::default()
        }
    }

    #[test]
    fn detail_tables_skip_ranked_combos_without_detail() {
        let report = analyze_bundle(&bundle(), &options()).unwrap();
        assert_eq!(report.top_ids.len(), 3 + 1);
        assert_eq!(report.top_ids[0].rsplit_once("|params=").unwrap().1, "a=1");
        assert_eq!(report.detail_top_ids.len(), 3);
        assert!(
            report
                .detail_top_ids
                .iter()
                .all(|id| !id.ends_with("|params=a=1"))
        );
        assert!(!report.calendar.is_empty());
    }

    #[test]
    fn dsr_trials_count_all_ok_rows_not_only_detail_rows() {
        let report = analyze_bundle(&bundle(), &options()).unwrap();
        let b = report
            .combos
            .iter()
            .find(|c| c.params == "b=1")
            .expect("b combo");
        let sig = b.significance.as_ref().expect("b significance");
        assert_eq!(sig.trials, 4);
        assert!(sig.trial_sr_variance.is_some_and(f64::is_finite));
    }

    #[test]
    fn zero_trials_is_rejected() {
        let opts = AnalyzeOptions {
            trials: Some(0),
            ..options()
        };
        assert!(analyze_bundle(&bundle(), &opts).is_err());
    }
}
