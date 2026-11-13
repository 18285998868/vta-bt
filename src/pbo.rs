//! PBO via CSCV (Bailey, Borwein, López de Prado & Zhu 2016, JCF).
//!
//! 输入: 同组(同币×同周期×同策略族)N条 fixed 权益曲线(逐 bar, 等长对齐后截齐)。
//! 输出: φ=PBO、logit 分布计数、IS 最优的 OOS 衰减。纯函数, 无 IO;
//! IO 装配见 `--pbo` (读 detail fixedcurve 文件, 按组调用本模块)。
//!
//! 口径说明:
//! - Sharpe 与引擎 `curve_stats` 同式: 逐 bar **对数收益**样本方差(ddof=1)×√bpy,
//!   逐 bar 收益由 `engine::bar_log_returns` 统一提供(含权益≤0 记 0 的定义域保护),
//!   故 CSCV 排序与引擎 `sharpe_ratio` 同源。
//! - OOS 相对排名 ω̄ = (OOS 弱于 n* 者数 + 1)/(N+1)(原文秩, 秩首为最差):
//!   n* OOS 居首 → ω̄=N/(N+1) → λ>0 表一致; n* OOS 垫底 → λ<0 表过拟合。
//!   故 ω̄∈[1/(N+1), N/(N+1)], logit 有限。并列按差处理(保守: 拿不到高排名)。
//! - φ = λ<0 的占比(原文 ∫_{-∞}^{0}f(λ)dλ; λ=0 的测度为零, < 与 ≤ 一致)。

/// 单组 PBO 结果(可直接印表)。
#[derive(Debug, Clone)]
pub struct PboResult {
    /// 组内策略数 N。
    pub n_strategies: usize,
    /// 参与计算的 bar 数 T(已按 S 截齐)。
    pub t_bars: usize,
    /// CSCV 分块数 S。
    pub s_blocks: usize,
    /// 枚举出的划分数 C(S, S/2)。
    pub n_logits: usize,
    /// φ: IS 最优 OOS 跑输中位数的概率估计。>0.05 按 Neyman-Pearson 惯例拒绝该模型。
    pub pbo: f64,
    /// **全样本回看**挑出的最优策略下标。
    ///
    /// 口径警告: 这是拿全样本数据挑出来的, 天然乐观, **不是业绩证据**。
    /// 它与 [`Self::degradation`] 依据的"最优"**不是同一个**:
    /// 后者用的是每个 CSCV 划分的 IS 最优(`r_is` 的 argmax), 随划分变化。
    /// 原实现把两者都叫 "is_best", 读表的人会以为同一件事。
    pub full_sample_best_idx: usize,
    /// 全样本回看的最优 Sharpe(年化, **乐观值**)。
    pub full_sample_best_sharpe: f64,
    /// 每个 CSCV 划分中"IS 最优"的策略, 其 OOS Sharpe 减 IS Sharpe 的均值
    /// (年化, 一般为负 = 过拟合的典型形态)。
    ///
    /// 注意: 这是**逐步 IS 选择**下的衰减, 与 [`Self::full_sample_best_sharpe`]
    /// 那个"一次全样本选择"的口径不同, 两者不可互相印证。
    pub degradation: f64,
}

/// S 选择: 块长 ≥32 bar; T<32 拒绝(None)。S=16 时 C(16,8)=12870 个 logit。
#[must_use]
pub fn pick_s(t_bars: usize) -> Option<usize> {
    if t_bars >= 512 {
        Some(16)
    } else if t_bars >= 128 {
        Some(8)
    } else if t_bars >= 32 {
        Some(4)
    } else {
        None
    }
}

/// 每块实际覆盖的**收益笔数**(与 [`cscv`] 的累加映射严格同源)。
///
/// 累加映射是 `b = i * s / (t - 1)`(整除下取整), 于是块 `b` 覆盖的收益下标
/// `i` 是区间 `[b·m/s, (b+1)·m/s)`, 其中 `m = t - 1`(首 bar 无收益)。
/// 该区间内的**整数**个数为
///
/// ```text
/// count(b) = ceil((b+1)·m/s) − ceil(b·m/s)
/// ```
///
/// **为什么必须用 ceil 而不是整除差**: 整除差 `b·m/s .. (b+1)·m/s` 的边界是
/// **floor** 语义, 与上面 floor 分块的边界差一, 在 `s >= 2` 时**永远不相等**
/// —— 因为 `t` 是 `s` 的倍数, 故 `s ∤ (t−1)`。实测 `t=32,s=4` 时真实笔数为
/// `[8,8,8,7]`, 整除差给出的是 `[7,8,8,8]`。
///
/// 笔数 `cnt` 是 Sharpe 的分母(均值 `sum/cnt`、方差自由度 `cnt−1`), 用错笔数
/// 会同时污染两者: 均值被放大/缩小(上例 15 vs 16 ⇒ 6.7%), 方差分母同步错;
/// 更糟的是 `cnt` 被**低估**时 `sq − sum²/cnt` 可能变负, 于是方差被夹到 0,
/// 该策略的 IS Sharpe 被**静默强制为 0** —— 低波动策略(最可能成为 IS 最优的
/// 那种)首当其冲。最终 `r_is`/`r_oos` 的 argmax 被改变, φ 被算错。
///
/// 不变量: `count(b)` 之和恒为 `t − 1`(总收益笔数)。
///
/// # Examples
/// ```
/// use vta::pbo::block_counts;
/// assert_eq!(block_counts(32, 4), vec![8, 8, 8, 7]);
/// assert_eq!(block_counts(512, 2), vec![256, 255]);
/// ```
#[must_use]
pub fn block_counts(t: usize, s: usize) -> Vec<usize> {
    if s == 0 {
        return Vec::new();
    }
    // `t < 2` 时无任何收益(首 bar 无收益), 全部块笔数为 0。
    let m = t.saturating_sub(1);
    if m == 0 {
        return vec![0; s];
    }
    (0..s)
        .map(|b| ((b + 1) * m).div_ceil(s) - (b * m).div_ceil(s))
        .collect()
}

/// 两遍法 Sharpe 的**参照实现**(先物化收益数组, 再单遍统计)。
///
/// 仅测试用: 它是 [`sharpe_of_curve`] 的正确性基准 —— 刻意用与后者**不同**的写法
/// (物化 `Vec` 后再索引), 使"两者同时写错同一处"不可能发生。
#[cfg(test)]
fn sharpe_of(rets: &[f64], bpy: f64) -> f64 {
    let n = rets.len();
    if n < 2 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / n as f64;
    let var = rets.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / (n as f64 - 1.0);
    if var <= 0.0 {
        0.0
    } else {
        mean / var.sqrt() * bpy.sqrt()
    }
}

/// 曲线 → 全样本 Sharpe。**不物化收益数组**, 但与「物化后两遍统计」逐位一致。
///
/// 口径与 [`sharpe_of`] 完全相同(两遍法: 先均值、再离差平方和), 只是收益序列
/// 由 [`crate::engine::bar_log_returns`] 惰性产出而非落成 `Vec`。
///
/// 为什么要这样: CSCV 的输入是 T×N 的曲线矩阵(1m 周期 × 500 组 ≈ 24 GB),
/// 再物化一份等大的收益数组就把峰值内存翻倍。逐位一致性由
/// `sharpe_of_curve_matches_materialized_reference` 锁定。
fn sharpe_of_curve(curve: &[f64], bpy: f64) -> f64 {
    let rets = crate::engine::bar_log_returns(curve);
    let n = rets.len();
    if n < 2 {
        return 0.0;
    }
    let mean = rets.clone().sum::<f64>() / n as f64;
    let var = rets.map(|r| (r - mean) * (r - mean)).sum::<f64>() / (n as f64 - 1.0);
    if var <= 0.0 {
        0.0
    } else {
        mean / var.sqrt() * bpy.sqrt()
    }
}

/// curves\[j\] = 第 j 条策略的 fixed 权益曲线(等长)。bpy = 该 TF 年化 bar 数。
/// S 必须为偶数且 T 可被 S 整除(尾部余 bar 丢弃, 与原文按行等切一致)。
///
/// # Errors
/// `s` 不是 `2..=16` 的偶数、策略数 N < 2、或可用 bar 数 T < S 时返回
/// `AppError`(crate::error::AppError) —— 这三种都是**调用方的口径错误**,
/// 不该让整轮 PBO 实算崩在一条 assert 上。
pub fn cscv(curves: &[impl AsRef<[f64]>], s: usize, bpy: f64) -> crate::error::Result<PboResult> {
    if !(2..=16).contains(&s) || !s.is_multiple_of(2) {
        return Err(crate::error::AppError::Msg(format!(
            "S 必须为 2..=16 的偶数, 当前为 {s}"
        )));
    }
    let n = curves.len();
    if n <= 1 {
        return Err(crate::error::AppError::Msg(format!(
            "策略数 N 必须 > 1, 当前为 {n}"
        )));
    }
    let t = curves.iter().map(|c| c.as_ref().len()).min().unwrap_or(0);
    if t < s {
        return Err(crate::error::AppError::Msg(format!(
            "T = {t} 必须 >= S = {s}"
        )));
    }
    let t = t - (t % s);

    // 每(策略,块): 收益和/平方和/笔数。Sharpe 只需这两矩, 组合枚举时 O(1) 聚合。
    //
    // 收益**不物化**: 逐点经 `bar_log_returns` 惰性产出并直接累加到分块矩里。
    // 原实现先落一份 T×N 的 `Vec<Vec<f64>>` 再累加, 在 1m×500 规模上多占 4 GB。
    let mut sums = vec![vec![0.0; s]; n];
    let mut sqs = vec![vec![0.0; s]; n];
    for (j, c) in curves.iter().enumerate() {
        for (i, v) in crate::engine::bar_log_returns(&c.as_ref()[..t]).enumerate() {
            let b = (i * s / (t - 1)).min(s - 1);
            sums[j][b] += v;
            sqs[j][b] += v * v;
        }
    }
    // 每块**实际**覆盖的收益笔数(与上面的累加映射**严格同源**)。
    let counts = block_counts(t, s);
    let sharpe_of_blocks = |blocks: &[usize]| -> Vec<f64> {
        (0..n)
            .map(|j| {
                let mut sum = 0.0;
                let mut sq = 0.0;
                let mut cnt = 0usize;
                for &b in blocks {
                    sum += sums[j][b];
                    sq += sqs[j][b];
                    cnt += counts[b];
                }
                if cnt < 2 {
                    return 0.0;
                }
                let mean = sum / cnt as f64;
                let var = (sq - sum * sum / cnt as f64) / (cnt as f64 - 1.0);
                if var <= 0.0 {
                    0.0
                } else {
                    mean / var.sqrt() * bpy.sqrt()
                }
            })
            .collect()
    };

    // 全样本最优(报告用)。
    let full: Vec<f64> = (0..n)
        .map(|j| sharpe_of_curve(&curves[j].as_ref()[..t], bpy))
        .collect();
    // `total_cmp` 而非 `partial_cmp().unwrap()`: 后者在任一 Sharpe 为 NaN 时 panic。
    // NaN 不是理论风险 —— 方差计算里任何一次溢出/Inf 都会传染到 mean 与 sqrt。
    // `total_cmp` 给出全序且对 NaN 无害(NaN 排在 +∞ 之后), 故取不到最优时只会
    // 静默选到某个确定的下标, 而不是让整轮 PBO 崩掉。
    let is_best_idx = full
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .unwrap_or(0);

    // C(S,S/2) 位掩码枚举。
    let half = s / 2;
    let mut neg = 0usize;
    let mut total = 0usize;
    let mut deg_sum = 0.0;
    let all: u32 = (1u32 << s) - 1;
    let mut mask = (1u32 << half) - 1;
    while mask <= all {
        let mut in_c = Vec::with_capacity(half);
        let mut out_c = Vec::with_capacity(half);
        for b in 0..s {
            if mask & (1u32 << b) != 0 {
                in_c.push(b);
            } else {
                out_c.push(b);
            }
        }
        let r_is = sharpe_of_blocks(&in_c);
        // 同上: `total_cmp` 避免 NaN Sharpe 触发 panic。
        let best = r_is
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let r_oos = sharpe_of_blocks(&out_c);
        let worse = r_oos.iter().filter(|v| **v < r_oos[best]).count();
        let omega = (worse + 1) as f64 / (n + 1) as f64;
        let lambda = (omega / (1.0 - omega)).ln();
        if lambda < 0.0 {
            neg += 1;
        }
        total += 1;
        deg_sum += r_oos[best] - r_is[best];
        // Gosper hack: 下一个同 popcount 掩码。
        let c = mask.isolate_lowest_one();
        let r = mask + c;
        if r == 0 {
            break;
        }
        mask = (((r ^ mask) >> 2) / c) | r;
    }

    Ok(PboResult {
        n_strategies: n,
        t_bars: t,
        s_blocks: s,
        n_logits: total,
        pbo: neg as f64 / total.max(1) as f64,
        full_sample_best_idx: is_best_idx,
        full_sample_best_sharpe: full[is_best_idx],
        degradation: deg_sum / total.max(1) as f64,
    })
}

/// 一次分组筛选的统计, 用于 `warn!` 与单测断言。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CurveSelection {
    /// 最终参与 CSCV 的曲线数。
    kept: usize,
    /// `status != ok` 被筛掉的曲线数。
    dropped_status: usize,
    /// 时间列空/不等长/非严格递增被筛掉的曲线数。
    dropped_invalid: usize,
    /// 时间网格与选中 cohort 不同而被筛掉的曲线数。
    dropped_other_window: usize,
    /// 同 params 重复被去掉的曲线数。
    dropped_duplicate: usize,
    /// 筛掉非法状态/时间轴后存在多少个不同的时间网格。
    cohorts: usize,
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
fn select_curves(members: &[crate::output::FixedCurve]) -> (Vec<(&str, &[f64])>, CurveSelection) {
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
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    // 确定性伪随机(LCG), 测试不依赖 rand crate。
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f64) / (u64::MAX >> 33) as f64
        }
    }

    fn noise_curves(n: usize, t: usize, seed: u64) -> Vec<Vec<f64>> {
        let mut rng = Lcg(seed);
        (0..n)
            .map(|_| {
                let mut eq = 10000.0;
                let mut c = vec![eq];
                for _ in 1..t {
                    eq *= 1.0 + (rng.next() - 0.5) * 0.02;
                    c.push(eq);
                }
                c
            })
            .collect()
    }

    #[test]
    fn test_pick_s_thresholds() {
        assert_eq!(pick_s(31), None);
        assert_eq!(pick_s(32), Some(4));
        assert_eq!(pick_s(127), Some(4));
        assert_eq!(pick_s(128), Some(8));
        assert_eq!(pick_s(511), Some(8));
        assert_eq!(pick_s(512), Some(16));
    }

    #[test]
    fn test_pure_noise_pbo_high() {
        // 纯噪声 N=30: IS 最优大概率 OOS 跑输中位数 → φ 显著 >0.05。
        let curves = noise_curves(30, 400, 7);
        let r = cscv(&curves, 4, 252.0).expect("合法口径的 CSCV 必须成功");
        assert_eq!(r.n_logits, 6); // C(4,2)
        assert!(r.pbo > 0.05, "noise φ={}", r.pbo);
    }

    #[test]
    fn test_planted_edge_pbo_low() {
        // 植入真边际: 策略0每 bar +0.004 确定漂移, 其余噪声 → IS 最优稳定居首, φ 应很低。
        let t = 400;
        let mut curves = noise_curves(20, t, 42);
        let mut rng = Lcg(99);
        let mut eq = 10000.0;
        let mut c = vec![eq];
        for _ in 1..t {
            eq *= 1.0 + 0.004 + (rng.next() - 0.5) * 0.004;
            c.push(eq);
        }
        curves[0] = c;
        let r = cscv(&curves, 4, 252.0).expect("合法口径的 CSCV 必须成功");
        assert_eq!(
            r.full_sample_best_idx, 0,
            "全样本最优应是那条植入漂移的曲线"
        );
        assert!(r.full_sample_best_sharpe > 0.0);
        assert!(r.pbo < 0.2, "edge φ={}", r.pbo);
    }

    /// L14: `full_sample_best_*` 确实是**全样本**口径 —— 用同一公式独立重算一遍
    /// 交叉验证。字段重命名正是为了让读者不再把它与 `degradation` 依赖的
    /// "每划分 IS 最优"混为一谈; 本测试锁定前者的确切含义不被后续改动悄悄改掉。
    #[test]
    fn full_sample_best_is_really_full_sample_sharpe() {
        let t = 256;
        let s_blocks = 8;
        let bpy = 252.0;
        let n = 12;
        let mut curves = Vec::new();
        for j in 0..n {
            let mut c = vec![10_000.0];
            let mut eq = 10_000.0;
            for i in 1..t {
                let drift = match j {
                    0 if i < t / 2 => 0.0015,
                    0 => -0.0005,
                    1 if i < t / 2 => -0.0005,
                    1 => 0.0015,
                    _ => 0.0,
                };
                let noise = ((j * 7919 + i * 104_729) % 1000) as f64 / 100_000_000.0 - 5e-6;
                eq *= 1.0 + drift + noise;
                c.push(eq);
            }
            curves.push(c);
        }
        let r = cscv(&curves, s_blocks, bpy).expect("合法口径的 CSCV 必须成功");

        // 独立重算全样本 Sharpe(argmax 用稳定规则: 取最大值的首个下标)
        let full: Vec<f64> = curves
            .iter()
            .map(|c| {
                let rets: Vec<f64> = crate::engine::bar_log_returns(c).collect();
                sharpe_of(&rets, bpy)
            })
            .collect();
        let argmax = full
            .iter()
            .enumerate()
            .fold((0usize, f64::NEG_INFINITY), |(bi, bv), (i, &v)| {
                if v > bv { (i, v) } else { (bi, bv) }
            })
            .0;
        assert_eq!(
            r.full_sample_best_idx, argmax,
            "字段必须仍是全样本 Sharpe 最大者"
        );
        assert!((r.full_sample_best_sharpe - full[argmax]).abs() < 1e-12);
        assert!(r.pbo.is_finite() && (0.0..=1.0).contains(&r.pbo));
    }

    #[test]
    fn test_sharpe_matches_engine_formula() {
        // 常数收益序列Sharpe 应为 0(方差为零); 线性序列与手算一致。
        assert_eq!(sharpe_of(&[0.01, 0.01, 0.01], 252.0), 0.0);
        let r = sharpe_of(&[0.01, -0.01, 0.01, -0.01], 252.0);
        assert!(r.abs() < 1e-9, "{r}");
    }

    /// 回归: NaN Sharpe **不得**让 CSCV panic。
    ///
    /// 可触发场景: 全样本/分块 argmax 原用 `partial_cmp(..).unwrap()`, 遇 NaN 即
    /// `None` 解包 panic。现用 `total_cmp`(全序, 无 None)。
    #[test]
    fn nan_sharpe_argmax_path_is_total() {
        // 直接验证 argmax 所依赖的比较语义: 全序 ⇒ 任意输入(含 NaN)都有最大值。
        for xs in [
            vec![f64::NAN, 2.0, 1.0],
            vec![1.0, f64::NAN],
            vec![f64::NEG_INFINITY, f64::NAN, f64::INFINITY],
        ] {
            let best = xs.iter().max_by(|a, b| a.total_cmp(b)).copied();
            assert!(best.is_some(), "{xs:?} 应有确定的最大值");
        }
    }

    /// `total_cmp` 在含 NaN 的序列上给出**全序**且不 panic, 使 argmax 必定有结果。
    ///
    /// 语义提示: 正 NaN 在 `total_cmp` 下排在**最大**一侧, 故 `max_by` 遇到 NaN
    /// 会选中它。这与 `partial_cmp(..).unwrap()` 的差别不在"选谁", 而在于前者
    /// **不会 panic** —— 见 `nan_inputs_do_not_panic_in_cscv` 的说明。
    #[test]
    fn total_cmp_orders_nan_without_panicking() {
        let mut v = [1.0, f64::NAN, 3.0, f64::NEG_INFINITY];
        // 旧写法 `a.partial_cmp(b).unwrap()` 在此会 panic(None 解包)。
        v.sort_by(f64::total_cmp);
        assert_eq!(v[0], f64::NEG_INFINITY, "-inf 排最前");
        assert_eq!(v[1], 1.0);
        assert_eq!(v[2], 3.0);
        assert!(v[3].is_nan(), "正 NaN 排在末尾(全序约定)");

        // 全序 ⇒ argmax 必有结果, 不再因 None 解包而 panic。
        let best = [f64::NAN, 2.0, 1.0]
            .iter()
            .max_by(|a, b| a.total_cmp(b))
            .copied();
        assert!(best.is_some(), "含 NaN 时 max_by 仍须给出结果");
        assert!(best.unwrap().is_nan() || best.unwrap() == 2.0);
    }

    /// 非有限输入**不得**让 CSCV panic, 且返回值必须可读。
    ///
    /// 说明(重要): 本用例注入 `NaN` / `Inf` 曲线, 但 NaN 实际**不会**传播到
    /// Sharpe —— `bar_log_returns` 复用 `engine::bar_log_returns`, 该函数对两侧非有限值
    /// 一律记 0。因此这里真正锁定的是"退化输入下 cscv 稳定返回", 而非"NaN 被隔离"。
    /// 换成 `partial_cmp().unwrap()` 时, 若未来 `sharpe_of` 的守卫被放宽, 本用例
    /// 才会真正覆盖到 panic 路径。
    #[test]
    fn nan_inputs_do_not_panic_in_cscv() {
        let t = 64;
        let s = 4;
        let mut curves = noise_curves(10, t, 5);
        curves[3] = vec![f64::NAN; t];
        curves[7] = vec![f64::INFINITY; t];
        let r = cscv(&curves, s, 252.0).expect("合法口径的 CSCV 必须成功");
        assert!(r.pbo.is_finite(), "pbo 必须有限: {}", r.pbo);
        assert!((0.0..=1.0).contains(&r.pbo));
        assert!(r.degradation.is_finite(), "decay: {}", r.degradation);
        assert_eq!(r.n_strategies, 10);
    }

    /// `cscv` 的非法口径必须**返回 Err**, 而不是 panic。
    ///
    /// 可触发场景: `cscv` 原先用三条 `assert!` 兜底, 而它是 crate 的**公共 API**
    /// (`vta::pbo::cscv`) —— 下游传 `s = 0` / `s = 3` / 单条曲线即崩, 且
    /// 文档里既无 `# Panics` 也无 `# Errors`, 调用方无从预知失败契约。
    #[test]
    fn cscv_rejects_invalid_arguments_instead_of_panicking() {
        let curves = noise_curves(4, 64, 3);

        // S 非法: 0 / 1 / 奇数 / 超过 16。
        for s in [0usize, 1, 3, 17, 100] {
            let e = cscv(&curves, s, 252.0).expect_err(&format!("S={s} 必须被拒, 而不是 panic"));
            assert!(
                format!("{e}").contains("S 必须"),
                "错误消息应说明是 S 的问题: {e}"
            );
        }

        // N 不足: 只有一条曲线无法做 CSCV(需要至少 2 条做相对排名)。
        let one = noise_curves(1, 64, 3);
        let e = cscv(&one, 4, 252.0).expect_err("N=1 必须被拒");
        assert!(format!("{e}").contains("N 必须"), "{e}");

        // T 不足: 曲线长度 < S。
        let short = noise_curves(4, 3, 3);
        let e = cscv(&short, 8, 252.0).expect_err("T<S 必须被拒");
        assert!(format!("{e}").contains("T ="), "{e}");
    }

    /// `sharpe_of_curve`(不物化收益数组)必须与「物化后两遍统计」**逐位一致**。
    ///
    /// 这是"省掉 T×N 收益矩阵"这一优化的**安全网**: 口径一变(例如改成
    /// `sum(x^2) - n·mean^2` 的单遍形式), 数值就会漂, φ 与 degradation 随之漂。
    #[test]
    fn sharpe_of_curve_matches_materialized_reference() {
        let bpy = 252.0;
        for c in noise_curves(6, 128, 11) {
            let rets: Vec<f64> = crate::engine::bar_log_returns(&c).collect();
            let expected = sharpe_of(&rets, bpy);
            let got = sharpe_of_curve(&c, bpy);
            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "不物化版本必须与物化版本逐位一致: {got} vs {expected}"
            );
        }
    }

    /// `cscv` 接受**借用**的曲线切片, 且结果与传 `Vec` 完全一致。
    ///
    /// 这条锁定的是 API 形状: `run_pbo_report` 现在零拷贝地把 `Vec<f64>` 借成
    /// `&[f64]` 传进来, 省掉一整份 T×N 克隆(1m×500 规模 ≈ 4 GB)。
    #[test]
    fn cscv_accepts_borrowed_slices_with_identical_result() {
        let owned = noise_curves(12, 256, 77);
        let borrowed: Vec<&[f64]> = owned.iter().map(Vec::as_slice).collect();
        let a = cscv(&owned, 4, 252.0).expect("owned 应成功");
        let b = cscv(&borrowed, 4, 252.0).expect("borrowed 应成功");
        assert_eq!(a.pbo.to_bits(), b.pbo.to_bits());
        assert_eq!(a.degradation.to_bits(), b.degradation.to_bits());
        assert_eq!(a.full_sample_best_idx, b.full_sample_best_idx);
        assert_eq!(
            a.full_sample_best_sharpe.to_bits(),
            b.full_sample_best_sharpe.to_bits()
        );
    }

    /// **核心不变量**: 每块笔数必须与 `cscv` 的实际累加映射**逐块相等**。
    ///
    /// 这条直接锁死曾经的缺陷: 笔数曾用 `b·m/s .. (b+1)·m/s` 的整除差
    /// (floor 边界)推算, 而累加用的是 floor 分块 `b = i·s/(t−1)`(ceil 边界),
    /// 两者在 `s ≥ 2` 时**永远不相等** ⇒ Sharpe 分母错 ⇒ φ 被算错。
    ///
    /// 参照实现是**暴力计数**: 把每个收益真的投进它所属的块再数 `len()`,
    /// 不依赖任何边界推导 —— 故"两边同时写错"不可能发生。
    #[test]
    fn block_counts_match_brute_force_accumulation() {
        for t in [2usize, 4, 8, 16, 32, 64, 128, 256, 512, 1000] {
            for s in [2usize, 4, 8, 16] {
                if t % s != 0 {
                    continue;
                }
                let mut brute = vec![0usize; s];
                for i in 0..(t - 1) {
                    let b = (i * s / (t - 1)).min(s - 1);
                    brute[b] += 1;
                }
                assert_eq!(
                    super::block_counts(t, s),
                    brute,
                    "t={t} s={s}: 块笔数必须与实际累加一致"
                );
            }
        }
    }

    /// 每块笔数之和恒等于总收益笔数 `t − 1`。
    #[test]
    fn block_counts_sum_to_total_return_count() {
        for t in [32usize, 64, 256, 512] {
            for s in [2usize, 4, 8, 16] {
                if t % s != 0 {
                    continue;
                }
                let sum: usize = super::block_counts(t, s).iter().sum();
                assert_eq!(sum, t - 1, "t={t} s={s}: 笔数总和必须等于 t-1");
            }
        }
    }

    /// 曾经的缺陷形态必须被本测试**区分出来**(= 测试有牙齿)。
    ///
    /// 旧口径用整除差推算笔数, 与真实累加**逐块不等**。若有人把
    /// [`super::block_counts`] 改回旧口径, 本测试立即失败。
    #[test]
    fn legacy_floor_based_counts_are_provably_wrong() {
        // 旧口径: `b·m/s .. (b+1)·m/s` 的整除差。
        let legacy = |t: usize, s: usize, b: usize| {
            let m = t - 1;
            (b + 1) * m / s - b * m / s
        };
        // t=32, s=4: 真实 [8,8,8,7], 旧口径 [7,8,8,8] —— 逐块都不等。
        assert_eq!(super::block_counts(32, 4), vec![8, 8, 8, 7]);
        assert_eq!(
            (0..4).map(|b| legacy(32, 4, b)).collect::<Vec<_>>(),
            vec![7, 8, 8, 8]
        );
        // 更普适地: 在 t、s 遍历下, 旧口径与真实口径**从不**完全相等。
        for t in [32usize, 64, 128, 256, 512] {
            for s in [2usize, 4, 8, 16] {
                if t % s != 0 {
                    continue;
                }
                let real = super::block_counts(t, s);
                let old: Vec<usize> = (0..s).map(|b| legacy(t, s, b)).collect();
                assert_ne!(real, old, "t={t} s={s}: 旧口径与真实口径不应相同");
            }
        }
    }

    /// CSCV 的**端到端参照实现**: 物化收益、按块**实际计数**、两遍法算方差。
    ///
    /// 与 [`cscv`] 的三处刻意差异(使"两边同时写错同一处"不可能发生):
    /// 1. 笔数由 `Vec::len()` 实际计数得到, 而非公式推算;
    /// 2. 收益**真的装进**每块的 `Vec`(而 `cscv` 只累加一阶/二阶矩);
    /// 3. 方差用两遍法(先均值再离差平方和), 而 `cscv` 用单遍代数式。
    ///
    /// `cscv` 的收益: 不物化 T×N 收益矩阵(1m×500 规模省 4 GB)。
    /// 本参照实现没有这个约束, 故可作为独立基准。
    fn cscv_materialized(curves: &[Vec<f64>], s: usize, bpy: f64) -> PboResult {
        let n = curves.len();
        let t = curves.iter().map(|c| c.len()).min().unwrap_or(0);
        let t = t - (t % s);
        let m = t - 1;

        // 物化: 每条曲线 → 每块的收益 Vec(笔数由 len() 实际给出)。
        let blocks: Vec<Vec<Vec<f64>>> = curves
            .iter()
            .map(|c| {
                let rets: Vec<f64> = crate::engine::bar_log_returns(&c[..t]).collect();
                let mut out: Vec<Vec<f64>> = vec![Vec::new(); s];
                for (i, v) in rets.iter().enumerate() {
                    let b = (i * s / m).min(s - 1);
                    out[b].push(*v);
                }
                out
            })
            .collect();

        let sharpe_of_blocks = |j: usize, sel: &[usize]| -> f64 {
            let mut vals: Vec<f64> = Vec::new();
            for &b in sel {
                vals.extend_from_slice(&blocks[j][b]);
            }
            sharpe_of(&vals, bpy)
        };

        let half = s / 2;
        let mut neg = 0usize;
        let mut total = 0usize;
        let mut deg_sum = 0.0;
        let all: u32 = (1u32 << s) - 1;
        let mut mask = (1u32 << half) - 1;
        while mask <= all {
            let mut in_c: Vec<usize> = Vec::new();
            let mut out_c: Vec<usize> = Vec::new();
            for b in 0..s {
                if mask & (1u32 << b) != 0 {
                    in_c.push(b);
                } else {
                    out_c.push(b);
                }
            }
            let r_is: Vec<f64> = (0..n).map(|j| sharpe_of_blocks(j, &in_c)).collect();
            let best = r_is
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let r_oos: Vec<f64> = (0..n).map(|j| sharpe_of_blocks(j, &out_c)).collect();
            let worse = r_oos.iter().filter(|v| **v < r_oos[best]).count();
            let omega = (worse + 1) as f64 / (n + 1) as f64;
            let lambda = (omega / (1.0 - omega)).ln();
            if lambda < 0.0 {
                neg += 1;
            }
            total += 1;
            deg_sum += r_oos[best] - r_is[best];
            let c = mask.isolate_lowest_one();
            let r = mask + c;
            if r == 0 {
                break;
            }
            mask = (((r ^ mask) >> 2) / c) | r;
        }

        PboResult {
            n_strategies: n,
            t_bars: t,
            s_blocks: s,
            n_logits: total,
            pbo: neg as f64 / total.max(1) as f64,
            full_sample_best_idx: 0,
            full_sample_best_sharpe: 0.0,
            degradation: deg_sum / total.max(1) as f64,
        }
    }

    /// `cscv`(不物化收益)必须与「物化后逐块实际计数」的参照实现**一致**。
    ///
    /// 这条是分块口径的端到端安全网: 笔数口径一旦与实际累加脱节
    /// (曾经的 floor/ceil 边界错配), IS/OOS Sharpe 就被污染, φ 随之出错,
    /// 而**没有任何**现有测试会发现 —— 故此处用完全不同的写法做交叉验证。
    #[test]
    fn cscv_matches_materialized_reference() {
        let bpy = 252.0;
        for (n, t, s, seed) in [
            (10usize, 64usize, 4usize, 5u64),
            (12, 128, 4, 11),
            (10, 256, 8, 23),
            (8, 512, 16, 31),
            (16, 400, 4, 47),
        ] {
            let curves = noise_curves(n, t, seed);
            let got = cscv(&curves, s, bpy).expect("合法口径的 CSCV 必须成功");
            let want = cscv_materialized(&curves, s, bpy);
            assert_eq!(got.n_logits, want.n_logits, "n={n} t={t} s={s}");
            assert_eq!(
                got.pbo.to_bits(),
                want.pbo.to_bits(),
                "n={n} t={t} s={s}: φ 必须与参照实现逐位一致 ({} vs {})",
                got.pbo,
                want.pbo
            );
        }
    }

    /// `select_curves`: 只保留 ok + 同一时间网格 + 唯一 params 的曲线。
    ///
    /// 旧实现按 `min(len)` 截断: 两条 64 bar 的曲线会把 12 条 256 bar 的曲线
    /// 一起砍到 T=64, 且无任何提示。本测试锁定"选最大 cohort, 不截断"。
    #[test]
    fn pbo_selection_prefers_largest_time_cohort() {
        use crate::app::RunStatus;

        let mk = |params: String, status: RunStatus, start: i64, n: usize| {
            let times: Vec<i64> = (0..n as i64).map(|i| start + i * 60_000).collect();
            let values: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
            crate::output::FixedCurve {
                combo: "c".into(),
                symbol: "BTCUSDT".into(),
                timeframe: "1h".into(),
                strategy: "sma".into(),
                params,
                status,
                times,
                values,
            }
        };
        let mut members: Vec<_> = (0..12)
            .map(|i| mk(format!("p{i:02}"), RunStatus::Ok, 1_700_000_000, 256))
            .collect();
        // 两条来自另一数据窗口的短曲线: 不得把整组 T 拉到 64。
        members.push(mk("old1".into(), RunStatus::Ok, 1_600_000_000, 64));
        members.push(mk("old2".into(), RunStatus::Ok, 1_600_000_000, 64));
        // 非 ok 状态: 不参与排名。
        members.push(mk("flat".into(), RunStatus::NoTrades, 1_700_000_000, 256));
        // 重复 params: 只留一条。
        members.push(mk("p00".into(), RunStatus::Ok, 1_700_000_000, 256));

        let (selected, stats) = select_curves(&members);
        assert_eq!(stats.kept, 12, "12 条同网格唯一参数");
        assert_eq!(stats.dropped_status, 1);
        assert_eq!(stats.dropped_other_window, 2);
        assert_eq!(stats.dropped_duplicate, 1);
        assert_eq!(stats.cohorts, 2);
        assert_eq!(selected.len(), 12);
        assert_eq!(
            selected[0].1.len(),
            256,
            "必须保留 256 bar 的 cohort, 不得按 min(len)=64 截断"
        );
        let mut params: Vec<&str> = selected.iter().map(|(p, _)| *p).collect();
        params.sort_unstable();
        params.dedup();
        assert_eq!(params.len(), selected.len(), "params 必须唯一");
    }

    /// 分组键碰撞不得把不同时间网格合并: 键只有 `(len, first, last)`, 桶内必须
    /// 再用完整时间轴精确拆分。这里两条网格长度/首/尾完全相同, 仅内部不同。
    #[test]
    fn pbo_selection_splits_key_collisions_by_exact_time_axis() {
        use crate::app::RunStatus;

        let mk = |params: &str, times: &[i64]| crate::output::FixedCurve {
            combo: "c".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: params.into(),
            status: RunStatus::Ok,
            values: vec![100.0; times.len()],
            times: times.to_vec(),
        };
        // 两者 len=5 / first=0 / last=5, 但第 2 个时间点不同。
        let grid_a = [0_i64, 1, 2, 3, 5];
        let grid_b = [0_i64, 2, 3, 4, 5];
        let mut members: Vec<_> = (0..12).map(|i| mk(&format!("a{i}"), &grid_a)).collect();
        members.extend((0..2).map(|i| mk(&format!("b{i}"), &grid_b)));

        let (selected, stats) = select_curves(&members);
        assert_eq!(stats.cohorts, 2, "同键不同网格必须拆成两个 cohort");
        assert_eq!(stats.kept, 12, "只能选样本数更大的 grid_a");
        assert_eq!(stats.dropped_other_window, 2);
        assert!(selected.iter().all(|(p, _)| p.starts_with('a')));
    }

    /// 时间列空/不等长/非严格递增的曲线必须被筛掉, 不得参与 CSCV。
    #[test]
    fn pbo_selection_rejects_invalid_time_axes() {
        use crate::app::RunStatus;

        let mk = |params: &str, times: Vec<i64>| crate::output::FixedCurve {
            combo: "c".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: params.into(),
            status: RunStatus::Ok,
            values: vec![100.0; times.len()],
            times,
        };
        let good: Vec<i64> = (0..64).map(|i| 1_700_000_000 + i * 86_400).collect();
        let mut members: Vec<_> = (0..10)
            .map(|i| mk(&format!("p{i}"), good.clone()))
            .collect();
        members.push(mk("empty", Vec::new()));
        members.push(mk("dup_time", vec![1, 1, 2, 3]));
        members.push(mk("backwards", vec![3, 2, 1]));

        let (selected, stats) = select_curves(&members);
        assert_eq!(selected.len(), 10);
        assert_eq!(stats.dropped_invalid, 3);
        assert_eq!(stats.cohorts, 1);
    }

    /// 全部曲线都不是 ok 时返回空选择, 由调用方按 N<10 跳过并 warn。
    #[test]
    fn pbo_selection_without_ok_curves_is_empty() {
        use crate::app::RunStatus;

        let times: Vec<i64> = (0..64).map(|i| 1_700_000_000 + i * 86_400).collect();
        let members: Vec<_> = (0..12)
            .map(|i| crate::output::FixedCurve {
                combo: "c".into(),
                symbol: "BTCUSDT".into(),
                timeframe: "1d".into(),
                strategy: "sma".into(),
                params: format!("p{i}"),
                status: RunStatus::Bankrupt,
                values: vec![0.0; times.len()],
                times: times.clone(),
            })
            .collect();
        let (selected, stats) = select_curves(&members);
        assert!(selected.is_empty());
        assert_eq!(stats.kept, 0);
        assert_eq!(stats.dropped_status, 12);
        assert_eq!(stats.cohorts, 0);
    }
}
