use super::{PboResult, block_counts};

/// 曲线 → 全样本 Sharpe。**不物化收益数组**, 但与「物化后两遍统计」逐位一致。
///
/// 口径与物化收益数组的两遍法完全相同(两遍法: 先均值、再离差平方和), 只是收益序列
/// 由 [`crate::engine::bar_log_returns`] 惰性产出而非落成 `Vec`。
///
/// 为什么要这样: CSCV 的输入是 T×N 的曲线矩阵(1m 周期 × 500 组 ≈ 24 GB),
/// 再物化一份等大的收益数组就把峰值内存翻倍。逐位一致性由
/// `sharpe_of_curve_matches_materialized_reference` 锁定。
pub(super) fn sharpe_of_curve(curve: &[f64], bpy: f64) -> f64 {
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
