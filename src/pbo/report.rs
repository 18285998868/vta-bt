use super::{cscv, pick_s};

/// 一次分组筛选的统计, 用于 `warn!` 与单测断言。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct CurveSelection {
    /// 最终参与 CSCV 的曲线数。
    pub(super) kept: usize,
    /// `status != ok` 被筛掉的曲线数。
    pub(super) dropped_status: usize,
    /// 时间列空/不等长/非严格递增被筛掉的曲线数。
    pub(super) dropped_invalid: usize,
    /// 时间网格与选中 cohort 不同而被筛掉的曲线数。
    pub(super) dropped_other_window: usize,
    /// 同 params 重复被去掉的曲线数。
    pub(super) dropped_duplicate: usize,
    /// 筛掉非法状态/时间轴后存在多少个不同的时间网格。
    pub(super) cohorts: usize,
}

/// 从同组 (币, 周期, 策略) 曲线中选出可比较的 cohort。
///
/// 三条规则:
/// 1. 只保留 `status == ok`: `no_signals`/`no_trades`/`bankrupt` 的收益率没有
///    可比性, 混进来会稀释 φ;
/// 2. 只保留**时间网格完全一致**的曲线。旧实现按 `min(len)` 截断, 一条来自别的
///    数据窗口的短曲线会让整组 T 缩水, 且无任何提示;
/// 3. 同组内 params 重复时只留一条(优先较长曲线)。
///
/// 若存在多个时间网格, 选样本数最多的一个(并列时选 T 更大的), 其余曲线记入
/// [`CurveSelection::dropped_other_window`] 由调用方 `warn!`。
pub(super) fn select_curves(
    members: &[crate::output::FixedCurve],
) -> (Vec<(&str, &[f64])>, CurveSelection) {
    use std::collections::BTreeMap;

    let mut stats = CurveSelection::default();
    // 分组键只用 `(len, first, last)`: 它是 O(1) 的, 避免 `BTreeMap<&[i64], _>`
    // 在"所有曲线共享同一网格"时每次比较都扫完整个 T(O(N log N · T))。
    // 键碰撞(同长度/首尾但中间不同)在下面用**精确 slice 比较**再拆开, 正确性
    // 不依赖键的区分度; 真实数据里不同窗口通常在首/尾/长度上就已分开。
    let mut buckets: BTreeMap<(usize, i64, i64), Vec<&crate::output::FixedCurve>> = BTreeMap::new();
    for curve in members {
        if curve.status != crate::app::RunStatus::Ok {
            stats.dropped_status += 1;
            continue;
        }
        // 时间必须严格递增且与值等长: 否则"逐 bar 收益"没有意义。
        if curve.times.is_empty()
            || curve.times.len() != curve.values.len()
            || curve.times.windows(2).any(|w| w[0] >= w[1])
        {
            stats.dropped_invalid += 1;
            continue;
        }
        let key = (
            curve.times.len(),
            curve.times[0],
            curve.times[curve.times.len() - 1],
        );
        buckets.entry(key).or_default().push(curve);
    }

    // 键碰撞安全: 桶内再按完整时间轴精确分组。常见情况(全组同网格)每条曲线只
    // 与代表曲线比较一次, 总代价 O(N · T), 而不是 BTreeMap 的 O(N log N · T)。
    let mut cohorts: Vec<Vec<&crate::output::FixedCurve>> = Vec::new();
    for (_key, bucket) in buckets {
        let mut exact: Vec<Vec<&crate::output::FixedCurve>> = Vec::new();
        'next_curve: for curve in bucket {
            for cohort in &mut exact {
                if cohort[0].times == curve.times {
                    cohort.push(curve);
                    continue 'next_curve;
                }
            }
            exact.push(vec![curve]);
        }
        cohorts.extend(exact);
    }
    stats.cohorts = cohorts.len();
    let Some(mut cohort) = cohorts.into_iter().max_by_key(|curves| {
        (
            curves.len(),
            curves[0].times.len(),
            curves[0].times.first().copied().unwrap_or(0),
        )
    }) else {
        return (Vec::new(), stats);
    };
    stats.dropped_other_window =
        members.len() - stats.dropped_status - stats.dropped_invalid - cohort.len();
    cohort.sort_by(|a, b| {
        a.params
            .cmp(&b.params)
            .then_with(|| b.values.len().cmp(&a.values.len()))
    });
    let before = cohort.len();
    cohort.dedup_by(|a, b| a.params == b.params);
    stats.dropped_duplicate = before - cohort.len();
    stats.kept = cohort.len();
    (
        cohort
            .into_iter()
            .map(|c| (c.params.as_str(), c.values.as_slice()))
            .collect(),
        stats,
    )
}

/// `--pbo` 装配: 读报告目录(detail/ 优先, 回退目录本身)的 fixed 曲线,
/// 按 (币, 周期, 策略) 分组实算 CSCV 并印表。T×N 常驻内存, 1m×500 级约 24GB,
/// PBO 建议 TF≥30m 或 N≤500(见手册 Step5)。
///
/// 只读 `detail/manifest.toml` 索引列出的曲线(该索引由 detail 回测在结束时
/// 重写), 不做目录扫描 —— 否则参数已删除/上一轮遗留的曲线会静默参与 CSCV。
/// 只使用 `status == ok` 且共享同一时间网格的曲线; 具体取舍见 `select_curves`。
///
/// # Errors
/// 索引缺失/读取失败、某组的 CSCV 口径非法(`S` 非偶数 / N 不足 / T 不足)时
/// 返回 `AppError`(crate::error::AppError)。
pub fn run_pbo_report(dir: &std::path::Path) -> crate::error::Result<()> {
    use std::collections::BTreeMap;

    let detail = dir.join("detail");
    let src = if detail.is_dir() {
        detail
    } else {
        dir.to_path_buf()
    };
    let curves = crate::output::read_fixed_curves_from_manifest(&src)?;
    if curves.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "detail 索引中没有 fixed 曲线, 请先用 detail=true 且 fixed_curves=true 重跑: {}",
            src.display()
        )));
    }
    let mut groups: BTreeMap<(String, String, String), Vec<crate::output::FixedCurve>> =
        BTreeMap::new();
    for c in curves {
        groups
            .entry((c.symbol.clone(), c.timeframe.clone(), c.strategy.clone()))
            .or_default()
            .push(c);
    }
    println!(
        "\n{:<28} {:>6} {:>8} {:>3} {:>7} {:>7} {:<6} {:>13} {:>10}",
        "group", "N", "T", "S", "logits", "φ", "判据", "全样本最优SR*", "衰减"
    );
    println!("{}", "-".repeat(96));
    let mut computed = 0usize;
    let mut skipped = 0usize;
    for ((sym, tf, strat), members) in groups {
        let (selected, stats) = select_curves(&members);
        if stats.dropped_status > 0 {
            tracing::warn!(
                "{sym}/{tf}/{strat}: 筛掉 {} 条非 ok 曲线(no_signals/no_trades/bankrupt 不参与 PBO)",
                stats.dropped_status
            );
        }
        if stats.dropped_invalid > 0 {
            tracing::warn!(
                "{sym}/{tf}/{strat}: 筛掉 {} 条时间列非法(空/不等长/非递增)的曲线",
                stats.dropped_invalid
            );
        }
        if stats.cohorts > 1 {
            tracing::warn!(
                "{sym}/{tf}/{strat}: 检出 {} 个不同时间网格; 仅用样本数最多的 {} 条(丢弃 {} 条不同网格曲线)",
                stats.cohorts,
                stats.kept,
                stats.dropped_other_window
            );
        }
        if stats.dropped_duplicate > 0 {
            tracing::warn!(
                "{sym}/{tf}/{strat}: 去掉 {} 条重复 params 曲线",
                stats.dropped_duplicate
            );
        }
        if selected.len() < 10 {
            tracing::warn!(
                "{sym}/{tf}/{strat}: N={} <10, 跳过(名义N过小)",
                selected.len()
            );
            skipped += 1;
            continue;
        }
        let t = selected.first().map_or(0, |(_, v)| v.len());
        let Some(s) = pick_s(t) else {
            tracing::warn!("{sym}/{tf}/{strat}: T={t} <32, 跳过(块长不足)");
            skipped += 1;
            continue;
        };
        // 年化口径必须可靠: `bars_per_year` 对未知周期会**静默回退 365**,
        // 而这里的 `tf` 来自 Parquet metadata(不经过 `Config::validate`),
        // 故必须显式拦下未知周期 —— 否则 Sharpe 年化可错 100~1440 倍,
        // φ 与 degradation 被静默算错且无任何提示。
        if !crate::strategies::is_known_timeframe(&tf) {
            tracing::warn!("[跳过] {sym}/{tf}/{strat}: 未知周期, 年化口径不可靠");
            skipped += 1;
            continue;
        }
        // **零拷贝**: 直接借用各曲线切片, 不再克隆一份 T×N 矩阵
        // (1m×500 规模下那一份就是 4 GB)。`cscv` 内部会按 `S` 自行截齐
        // (`t - t % s`), 与原先在此处截齐后克隆的结果逐位相同。
        let mat: Vec<&[f64]> = selected.iter().map(|(_, v)| *v).collect();
        let bpy = crate::strategies::bars_per_year(&tf);
        let r = cscv(&mat, s, bpy)?;
        let verdict = if r.pbo < 0.05 { "通过" } else { "拒绝" };
        println!(
            "{:<28} {:>6} {:>8} {:>3} {:>7} {:>7.4} {:<6} {:>13.3} {:>+10.3}",
            format!("{sym}/{tf}/{strat}"),
            r.n_strategies,
            r.t_bars,
            r.s_blocks,
            r.n_logits,
            r.pbo,
            verdict,
            r.full_sample_best_sharpe,
            r.degradation
        );
        if r.n_logits < 70 {
            tracing::warn!("{sym}/{tf}/{strat}: logits<70, λ 分布稀疏, φ 估计误差大");
        }
        computed += 1;
    }
    // 参与计算的组数必须**显式**告知: 三条跳过分支都是 `continue` + `warn!`,
    // 若全部组被跳过, 表里一行都没有, 而收尾仍打印判据 —— 用户会以为"跑完了"。
    println!("\n共 {computed} 组参与计算 / {skipped} 组被跳过。");
    println!("判据: φ>0.05 按 Neyman-Pearson 惯例拒绝该模型(C3)。");
    println!("* 全样本最优SR 是**拿全样本挑出来的乐观值**, 不是业绩证据; 真实证据是 φ。");
    Ok(())
}
