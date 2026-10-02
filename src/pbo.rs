//! PBO via CSCV (Bailey, Borwein, López de Prado & Zhu 2016, JCF).
//!
//! 输入: 同组(同币×同周期×同策略族)N条 fixed 权益曲线(逐 bar, 等长对齐后截齐)。
//! 输出: φ=PBO、logit 分布计数、IS 最优的 OOS 衰减。纯函数, 无 IO;
//! IO 装配见 `--pbo` (读 detail fixedcurve 文件, 按组调用本模块)。
//!
//! 口径说明:
//! - Sharpe 与引擎 `curve_stats` 同式: 逐 bar 简单收益样本方差(ddof=1)×√bpy;
//!   唯一偏离: 上一 bar 权益 ≤0 时该 bar 收益记 0(引擎无此保护, 会出 inf 污染排名)。
//! - OOS 相对排名 ω̄ = (OOS 弱于 n* 者数 + 1)/(N+1)(原文秩, 秩首为最差):
//!   n* OOS 居首 → ω̄=N/(N+1) → λ>0 表一致; n* OOS 垫底 → λ<0 表过拟合。
//!   故 ω̄∈[1/(N+1), N/(N+1)], logit 有限。并列按差处理(保守: 拿不到高排名)。
//! - φ = λ<0 的占比(原文 ∫_{-∞}^{0}f(λ)dλ; λ=0 的测度为零, < 与 ≤ 一致)。

/// 单组 PBO 结果(可直接印表)。
#[derive(Debug, Clone)]
pub struct PboResult {
    pub n_strategies: usize,
    pub t_bars: usize,
    pub s_blocks: usize,
    pub n_logits: usize,
    /// φ: IS 最优 OOS 跑输中位数的概率估计。>0.05 按 Neyman-Pearson 惯例拒绝该模型。
    pub pbo: f64,
    /// 全样本 Sharpe 最大的策略下标。
    pub is_best_idx: usize,
    /// 全样本最优 Sharpe(年化)。
    pub is_best_sharpe: f64,
    /// 各组合 IS 最优的 OOS Sharpe 均值减 IS Sharpe 均值(性能衰减, 年化, 一般为负)。
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

/// 权益曲线 → 逐 bar 简单收益(与引擎同式; 上一 bar≤0 则记 0)。
fn to_returns(equity: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(equity.len().saturating_sub(1));
    for w in equity.windows(2) {
        let (prev, curr) = (w[0], w[1]);
        if prev <= 0.0 || !prev.is_finite() || !curr.is_finite() {
            out.push(0.0);
        } else {
            out.push(curr / prev - 1.0);
        }
    }
    out
}

/// curves[j] = 第 j 条策略的 fixed 权益曲线(等长)。bpy = 该 TF 年化 bar 数。
/// S 必须为偶数且 T 可被 S 整除(尾部余 bar 丢弃, 与原文按行等切一致)。
#[must_use]
pub fn cscv(curves: &[Vec<f64>], s: usize, bpy: f64) -> PboResult {
    assert!(
        s >= 2 && s.is_multiple_of(2) && s <= 16,
        "S 必须为 2..=16 的偶数"
    );
    let n = curves.len();
    assert!(n > 1, "策略数 N 必须 >1");
    let t = curves.iter().map(Vec::len).min().unwrap_or(0);
    assert!(t >= s, "T 必须 ≥S");
    let t = t - (t % s);
    let block = t / s;
    let _ = block;

    // 每(策略,块): 收益和/平方和/笔数。Sharpe 只需这两矩, 组合枚举时 O(1) 聚合。
    let rets: Vec<Vec<f64>> = curves.iter().map(|c| to_returns(&c[..t])).collect();
    let mut sums = vec![vec![0.0; s]; n];
    let mut sqs = vec![vec![0.0; s]; n];
    for (j, r) in rets.iter().enumerate() {
        for (i, &v) in r.iter().enumerate().take(t - 1) {
            let b = (i * s / (t - 1)).min(s - 1);
            sums[j][b] += v;
            sqs[j][b] += v * v;
        }
    }
    // 块 b 覆盖的收益下标(与上式同映射): Sharpe 分母用实际笔数(首 bar 无收益, T-1 个)。
    let block_n = |b: usize| {
        let lo = b * (t - 1) / s;
        let hi = (b + 1) * (t - 1) / s;
        hi - lo
    };
    let sharpe_of_blocks = |blocks: &[usize]| -> Vec<f64> {
        (0..n)
            .map(|j| {
                let mut sum = 0.0;
                let mut sq = 0.0;
                let mut cnt = 0usize;
                for &b in blocks {
                    sum += sums[j][b];
                    sq += sqs[j][b];
                    cnt += block_n(b);
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
    let full: Vec<f64> = (0..n).map(|j| sharpe_of(&rets[j], bpy)).collect();
    let is_best_idx = full
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
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
        let best = r_is
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
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

    PboResult {
        n_strategies: n,
        t_bars: t,
        s_blocks: s,
        n_logits: total,
        pbo: neg as f64 / total.max(1) as f64,
        is_best_idx,
        is_best_sharpe: full[is_best_idx],
        degradation: deg_sum / total.max(1) as f64,
    }
}

/// (币,周期,策略) → [(params, fixed 曲线)]。
type PboGroups = std::collections::BTreeMap<(String, String, String), Vec<(String, Vec<f64>)>>;

/// `--pbo` 装配: 读报告目录(detail/ 优先, 回退目录本身)的 fixed 曲线,
/// 按(币,周期,策略)分组实算 CSCV 并印表。T×N 常驻内存, 1m×500 级约 24GB,
/// PBO 建议 TF≥30m 或 N≤500(见手册 Step5)。
pub fn run_pbo_report(dir: &str) -> crate::error::Result<()> {
    use std::collections::BTreeMap;

    let detail = std::path::Path::new(dir).join("detail");
    let src = if detail.is_dir() {
        detail.to_string_lossy().into_owned()
    } else {
        dir.to_string()
    };
    let curves = crate::output::read_fixed_curves(&src)?;
    if curves.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "未找到 *_fixedcurve.parquet, 请先用 detail=true 重跑: {src}"
        )));
    }
    let mut groups: PboGroups = BTreeMap::new();
    for c in curves {
        groups
            .entry((c.symbol, c.timeframe, c.strategy))
            .or_default()
            .push((c.params, c.values));
    }
    println!(
        "\n{:<28} {:>6} {:>8} {:>3} {:>7} {:>7} {:<6} {:>9} {:>10}",
        "group", "N", "T", "S", "logits", "φ", "判据", "IS最优SR", "衰减"
    );
    println!("{}", "-".repeat(96));
    for ((sym, tf, strat), mut members) in groups {
        members.sort_by(|a, b| a.0.cmp(&b.0));
        if members.len() < 10 {
            println!(
                "{sym}/{tf}/{strat}: N={} <10, 跳过(名义N过小)",
                members.len()
            );
            continue;
        }
        let t = members.iter().map(|(_, v)| v.len()).min().unwrap_or(0);
        let Some(s) = pick_s(t) else {
            println!("{sym}/{tf}/{strat}: T={t} <32, 跳过(块长不足)");
            continue;
        };
        let t_use = t - (t % s);
        let mat: Vec<Vec<f64>> = members.iter().map(|(_, v)| v[..t_use].to_vec()).collect();
        let bpy = crate::strategies::bars_per_year(&tf);
        let r = cscv(&mat, s, bpy);
        let verdict = if r.pbo < 0.05 { "通过" } else { "拒绝" };
        println!(
            "{:<28} {:>6} {:>8} {:>3} {:>7} {:>7.4} {:<6} {:>9.3} {:>+10.3}",
            format!("{sym}/{tf}/{strat}"),
            r.n_strategies,
            r.t_bars,
            r.s_blocks,
            r.n_logits,
            r.pbo,
            verdict,
            r.is_best_sharpe,
            r.degradation
        );
        if r.n_logits < 70 {
            println!("  注: logits<70, λ 分布稀疏, φ 估计误差大");
        }
    }
    println!("\n判据: φ>0.05 按 Neyman-Pearson 惯例拒绝该模型(C3)。");
    Ok(())
}
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
        let r = cscv(&curves, 4, 252.0);
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
        let r = cscv(&curves, 4, 252.0);
        assert_eq!(r.is_best_idx, 0);
        assert!(r.pbo < 0.2, "edge φ={}", r.pbo);
    }

    #[test]
    fn test_sharpe_matches_engine_formula() {
        // 常数收益序列 Sharpe 应为 0(方差为零); 线性序列与手算一致。
        assert_eq!(sharpe_of(&[0.01, 0.01, 0.01], 252.0), 0.0);
        let r = sharpe_of(&[0.01, -0.01, 0.01, -0.01], 252.0);
        assert!(r.abs() < 1e-9, "{r}");
    }
}
