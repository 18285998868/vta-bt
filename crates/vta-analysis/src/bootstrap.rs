//! 平稳 bootstrap (Politis & Romano 1994) 与块长自动选择 (Politis & White 2004)。

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Serialize;

use crate::stats::{mean, quantile_sorted};

/// 一次平稳 bootstrap 的结果(年化夏普)。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BootstrapResult {
    /// 块长(bar 数)。
    pub block_len: usize,
    /// 重采样次数。
    pub resamples: usize,
    /// 重采样夏普的均值。
    pub mean_sr: f64,
    /// 重采样夏普的标准误。
    pub se_sr: f64,
    /// 百分位置信区间下界。
    pub ci_low: f64,
    /// 百分位置信区间上界。
    pub ci_high: f64,
}

/// 自动块长: 返回 `1..=n/2` 内的整数。
///
/// 采用 Politis & White (2004) 的 `b_opt` 公式:
///
/// ```text
/// b_opt = (2·G² / D)^(1/3) · n^(1/3)
/// G = Σ λ(k/M)·|k|·ρ(k)
/// D = (4/3)·(Σ λ(k/M)·ρ(k))²
/// ```
///
/// 其中 `M = 2·m_hat`, `m_hat` 是第一个使 `|ρ(k)| < 2√(log10(n)/n)` 对所有
/// `k ∈ [m_hat, √n]` 成立的滞后。λ 是梯形核。该选择对 AR(1) 类弱相关序列
/// 给出随 n 增长的块长; 对白噪声退化到 1 附近。
pub(crate) fn auto_block_len(x: &[f64]) -> usize {
    let n = x.len();
    if n < 8 {
        return 1;
    }
    let m = mean(x);
    let var = x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / n as f64;
    if var <= 0.0 || !var.is_finite() {
        return 1;
    }
    // P&W: 在 1..=K_n (K_n=ceil(√n)) 内找 m_hat, 但窗口 M=2·m_hat 需要
    // 自相关到 2·K_n, 故 rho 要算到 max_lag = min(2·K_n, n-1)。
    let k_n = ((n as f64).sqrt().ceil() as usize).min(n - 1).max(1);
    let max_lag = (2 * k_n).min(n - 1).max(1);
    let mut rho = vec![0.0; max_lag + 1];
    rho[0] = 1.0;
    for k in 1..=max_lag {
        let mut acc = 0.0;
        for t in k..n {
            acc += (x[t] - m) * (x[t - k] - m);
        }
        rho[k] = acc / (n as f64 * var);
    }
    let threshold = 2.0 * (10.0f64.log10() / n as f64).sqrt();
    let mut m_hat = k_n;
    'outer: for lag in 1..=k_n {
        for &r in &rho[lag..=k_n] {
            if r.abs() >= threshold {
                continue 'outer;
            }
        }
        m_hat = lag;
        break;
    }
    let cap = (n / 2).max(1);
    let big_m = (2 * m_hat).min(max_lag).max(1);
    let lambda = |t: f64| {
        let a = t.abs();
        if a <= 0.5 {
            1.0
        } else if a <= 1.0 {
            2.0 * (1.0 - a)
        } else {
            0.0
        }
    };
    let mut g = 0.0;
    let mut lr = 1.0;
    for (k, &rk) in rho.iter().enumerate().take(big_m + 1).skip(1) {
        let w = lambda(k as f64 / big_m as f64);
        g += w * k as f64 * rk;
        lr += 2.0 * w * rk;
    }
    g *= 2.0;
    let d = (4.0 / 3.0) * lr * lr;
    if d <= 0.0 || !d.is_finite() || !g.is_finite() {
        return 1;
    }
    let b = (2.0 * g * g / d).cbrt() * (n as f64).cbrt();
    if !b.is_finite() || b < 1.0 {
        return 1;
    }
    (b.round() as usize).clamp(1, cap)
}

/// 平稳 bootstrap 的夏普置信区间。
///
/// 每次重采样按几何块长(均值 `block_len`)有放回抽取收益; 块边界用
/// `rng < 1/block_len` 判定, 块内索引循环前进。夏普按与引擎相同的
/// 样本方差(ddof=1)计算, 再乘 `√bars_per_year` 年化。
///
/// 固定 `seed` 时结果逐位可复现。样本不足 20 或方差为 0 时返回 `None`。
pub(crate) fn stationary_bootstrap_sr(
    rets: &[f64],
    bars_per_year: f64,
    resamples: usize,
    block_len: Option<usize>,
    seed: u64,
    alpha: f64,
) -> Option<BootstrapResult> {
    if rets.len() < 20 || resamples == 0 || bars_per_year <= 0.0 || !bars_per_year.is_finite() {
        return None;
    }
    let n = rets.len();
    let l = block_len
        .unwrap_or_else(|| auto_block_len(rets))
        .clamp(1, n);
    let p_new = 1.0 / l as f64;
    let mut rng = StdRng::seed_from_u64(seed);
    let mut srs = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut idx = rng.random_range(0..n);
        // Welford, 与引擎一样用样本方差(ddof=1)。
        let mut count = 0usize;
        let mut mean = 0.0;
        let mut m2 = 0.0;
        for _ in 0..n {
            let v = rets[idx];
            count += 1;
            let delta = v - mean;
            mean += delta / count as f64;
            m2 += delta * (v - mean);
            if rng.random::<f64>() < p_new {
                idx = rng.random_range(0..n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        let var = if count > 1 {
            m2 / (count as f64 - 1.0)
        } else {
            0.0
        };
        let sr = if var > 0.0 {
            mean / var.sqrt() * bars_per_year.sqrt()
        } else {
            0.0
        };
        if sr.is_finite() {
            srs.push(sr);
        }
    }
    if srs.len() < 2 {
        return None;
    }
    srs.sort_by(f64::total_cmp);
    let mean_sr = mean(&srs);
    let var = srs
        .iter()
        .map(|v| (v - mean_sr) * (v - mean_sr))
        .sum::<f64>()
        / (srs.len() as f64 - 1.0);
    Some(BootstrapResult {
        block_len: l,
        resamples: srs.len(),
        mean_sr,
        se_sr: var.sqrt(),
        ci_low: quantile_sorted(&srs, alpha / 2.0),
        ci_high: quantile_sorted(&srs, 1.0 - alpha / 2.0),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn block_len_grows_with_autocorrelation() {
        let white: Vec<f64> = (0..1000)
            .map(|i| ((i * 37) % 101) as f64 / 100.0 - 0.5)
            .collect();
        let mut ar = vec![0.0; 1000];
        for i in 1..1000 {
            ar[i] = 0.8 * ar[i - 1] + (((i * 37) % 101) as f64 / 100.0 - 0.5);
        }
        let b_white = auto_block_len(&white);
        let b_ar = auto_block_len(&ar);
        assert!(b_ar > b_white, "white={b_white} ar={b_ar}");
        assert!(b_ar <= 500);
    }

    #[test]
    fn bootstrap_is_deterministic() {
        let rets: Vec<f64> = (0..300)
            .map(|i| ((i as f64) * 0.17).sin() * 0.01 + 0.0005)
            .collect();
        let a = stationary_bootstrap_sr(&rets, 365.0, 200, Some(10), 42, 0.05).unwrap();
        let b = stationary_bootstrap_sr(&rets, 365.0, 200, Some(10), 42, 0.05).unwrap();
        assert_eq!(a.ci_low.to_bits(), b.ci_low.to_bits());
        assert_eq!(a.ci_high.to_bits(), b.ci_high.to_bits());
    }
}
