//! 单组合显著性: Lo 标准误、PSR、DSR、MinTRL。
//!
//! 口径:
//! - `SR` 一律是**逐 bar** 夏普(非年化)。年化值 = `SR·√bars_per_year`;
//!   文献公式里的 `T` 是收益笔数, 不是 bar 数。
//! - 偏度/峰度用**总体矩**(除以 T), 峰度是原始峰度(正态 = 3)。
//! - PSR/DSR 的分母 `1 - γ3·SR + ((γ4-1)/4)·SR²` 必须为正; 否则返回
//!   `None`, 由调用方显式标记不可计算, 不静默夹到 0/1。

use crate::stats::{mean, normal_cdf, normal_quantile, raw_kurtosis, skewness, variance};

/// 收益序列的矩与逐 bar 夏普。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReturnMoments {
    /// 收益笔数 T。
    pub(crate) n: usize,
    /// 逐 bar 夏普 `mean/sd`。
    pub(crate) sr_bar: f64,
    /// 偏度。
    pub(crate) skew: f64,
    /// 原始峰度。
    pub(crate) kurtosis: f64,
}

/// 计算收益矩; 样本不足或方差为 0 时返回 `None`。
pub(crate) fn return_moments(rets: &[f64]) -> Option<ReturnMoments> {
    let n = rets.len();
    if n < 20 {
        return None;
    }
    let m = mean(rets);
    let var = variance(rets, m)?;
    if var <= 0.0 || !var.is_finite() {
        return None;
    }
    let sd = var.sqrt();
    let sr_bar = m / sd;
    if !sr_bar.is_finite() {
        return None;
    }
    Some(ReturnMoments {
        n,
        sr_bar,
        skew: skewness(rets).unwrap_or(0.0),
        kurtosis: raw_kurtosis(rets).unwrap_or(3.0),
    })
}

/// PSR 的分母平方项。非正或非有限时返回 `None`。
fn psr_denominator_sq(m: &ReturnMoments) -> Option<f64> {
    // 公式用的是被检验的 SR^, 不是基准 SR*。
    let d = 1.0 - m.skew * m.sr_bar + ((m.kurtosis - 1.0) / 4.0) * m.sr_bar * m.sr_bar;
    (d.is_finite() && d > 0.0).then_some(d)
}

/// `PSR(SR*) = Φ((SR - SR*)√(T-1) / √(1 - γ3·SR + ((γ4-1)/4)·SR²))`。
///
/// Bailey & López de Prado (2012), Journal of Risk 15(2)。
pub(crate) fn psr(m: &ReturnMoments, sr_benchmark: f64) -> Option<f64> {
    let d2 = psr_denominator_sq(m)?;
    let z = (m.sr_bar - sr_benchmark) * ((m.n - 1) as f64).sqrt() / d2.sqrt();
    let p = normal_cdf(z);
    p.is_finite().then_some(p)
}

/// Lo (2002) 的 iid 夏普标准误(逐 bar)。
///
/// `SE(SR) = √([1 + 0.5·SR² - γ3·SR + ((γ4-3)/4)·SR²] / T)`
///
/// 这里 `γ4` 是原始峰度, 故用 `(γ4 - 3)/4` 还原超额峰度项。自相关收益
/// 不满足 iid 假设, 应改用平稳 bootstrap 的置信区间; 本函数只作为
/// 解析近似与对照。
pub(crate) fn lo_standard_error(m: &ReturnMoments) -> Option<f64> {
    let sr2 = m.sr_bar * m.sr_bar;
    let v = 1.0 + 0.5 * sr2 - m.skew * m.sr_bar + ((m.kurtosis - 3.0) / 4.0) * sr2;
    if !(v > 0.0 && v.is_finite()) {
        return None;
    }
    Some((v / m.n as f64).sqrt())
}

/// DSR 的期望最大夏普(逐 bar)。
///
/// `SR*_0 = √V[SR_n] · [(1-γ)Φ⁻¹(1-1/N) + γΦ⁻¹(1-1/(N·e))]`
///
/// `γ` 为 Euler-Mascheroni 常数。`trials <= 1` 时按"无选择偏差"处理,
/// 返回 0 —— 此时 DSR 退化为 `PSR(0)`, 与 N=1 的极限一致。
pub(crate) fn expected_max_sr(sr_variance: f64, trials: usize) -> Option<f64> {
    if trials <= 1 || sr_variance < 0.0 || !sr_variance.is_finite() {
        return Some(0.0);
    }
    const EULER_GAMMA: f64 = std::f64::consts::EULER_GAMMA;
    let n = trials as f64;
    let q1 = normal_quantile(1.0 - 1.0 / n);
    let q2 = normal_quantile(1.0 - 1.0 / (n * std::f64::consts::E));
    if !q1.is_finite() || !q2.is_finite() {
        return None;
    }
    Some(sr_variance.sqrt() * ((1.0 - EULER_GAMMA) * q1 + EULER_GAMMA * q2))
}

/// 去偏夏普概率 `DSR = PSR(SR*_0)`。
pub(crate) fn dsr(m: &ReturnMoments, sr_variance: f64, trials: usize) -> Option<f64> {
    let sr0 = expected_max_sr(sr_variance, trials)?;
    psr(m, sr0)
}

/// 最小样本长度(收益笔数 `T`)。
///
/// `MinTRL = 1 + [1 - γ3·SR + ((γ4-1)/4)·SR²] · (Φ⁻¹(1-α)/(SR-SR*))²`
///
/// 返回的是**收益笔数**; 对应的 bar 数为 `MinTRL + 1`(首 bar 无收益)。
/// `SR <= SR*` 时所需样本无限, 返回 `None`。
pub(crate) fn min_trl(m: &ReturnMoments, sr_benchmark: f64, alpha: f64) -> Option<f64> {
    let excess = m.sr_bar - sr_benchmark;
    if excess <= 0.0 || !excess.is_finite() || alpha <= 0.0 || alpha >= 1.0 {
        return None;
    }
    let q = normal_quantile(1.0 - alpha);
    if !q.is_finite() {
        return None;
    }
    let d = 1.0 - m.skew * m.sr_bar + ((m.kurtosis - 1.0) / 4.0) * m.sr_bar * m.sr_bar;
    if d <= 0.0 || !d.is_finite() {
        return None;
    }
    let v = 1.0 + d * (q / excess).powi(2);
    v.is_finite().then_some(v)
}

/// Benjamini-Hochberg q 值, 与输入同序。
///
/// 只在**同一组试验**内使用: 跨 symbol/timeframe/strategy 混排会把不同原假设
/// 的 p 值放进同一个 FDR 控制, 结论没有意义。
pub(crate) fn benjamini_hochberg(p: &[f64]) -> Vec<f64> {
    let m = p.len();
    if m == 0 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..m).collect();
    idx.sort_by(|&a, &b| p[a].total_cmp(&p[b]));
    let mut q = vec![1.0; m];
    let mut running: f64 = 1.0;
    for (rank, &i) in idx.iter().enumerate().rev() {
        let val = p[i] * m as f64 / (rank + 1) as f64;
        running = running.min(val);
        q[i] = running.clamp(0.0, 1.0);
    }
    q
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn moments(rets: &[f64]) -> ReturnMoments {
        return_moments(rets).unwrap()
    }

    #[test]
    fn psr_at_zero_sr_is_half() {
        let m = ReturnMoments {
            n: 100,
            sr_bar: 0.0,
            skew: 0.0,
            kurtosis: 3.0,
        };
        assert!((psr(&m, 0.0).unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn psr_increases_with_sr() {
        let base = ReturnMoments {
            n: 252,
            sr_bar: 0.1,
            skew: 0.0,
            kurtosis: 3.0,
        };
        let higher = ReturnMoments {
            sr_bar: 0.2,
            ..base
        };
        assert!(psr(&higher, 0.0).unwrap() > psr(&base, 0.0).unwrap());
    }

    #[test]
    fn dsr_with_one_trial_equals_psr_zero() {
        let rets: Vec<f64> = (0..200)
            .map(|i| ((i as f64) * 0.37).sin() * 0.01 + 0.001)
            .collect();
        let m = moments(&rets);
        let p0 = psr(&m, 0.0).unwrap();
        let d = dsr(&m, 0.0, 1).unwrap();
        assert!((d - p0).abs() < 1e-15);
    }

    #[test]
    fn dsr_decreases_with_more_trials() {
        let rets: Vec<f64> = (0..500)
            .map(|i| ((i as f64) * 0.11).sin() * 0.01 + 0.0008)
            .collect();
        let m = moments(&rets);
        let d1 = dsr(&m, 0.0004, 2).unwrap();
        let d2 = dsr(&m, 0.0004, 100).unwrap();
        assert!(d2 < d1);
    }

    #[test]
    fn min_trl_finite_for_positive_sr() {
        let rets: Vec<f64> = (0..300)
            .map(|i| ((i as f64) * 0.19).sin() * 0.01 + 0.001)
            .collect();
        let m = moments(&rets);
        let t = min_trl(&m, 0.0, 0.05).unwrap();
        assert!(t.is_finite() && t > 0.0);
    }

    #[test]
    fn bh_matches_hand_example() {
        // p = [0.01, 0.04, 0.03, 0.20], m=4.
        // 排序: 0.01(1), 0.03(2), 0.04(3), 0.20(4)
        // 逐步取 min 后: 0.04, 0.0533, 0.0533, 0.20
        let q = benjamini_hochberg(&[0.01, 0.04, 0.03, 0.20]);
        assert!((q[0] - 0.04).abs() < 1e-12);
        assert!((q[1] - 0.053_333_333_333_333_33).abs() < 1e-12);
        assert!((q[2] - 0.053_333_333_333_333_33).abs() < 1e-12);
        assert!((q[3] - 0.20).abs() < 1e-12);
    }
}
