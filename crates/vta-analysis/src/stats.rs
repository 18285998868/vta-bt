//! 基础统计: 矩、分位、正态 CDF/分位与稳定哈希。
//!
//! PSR/DSR/MinTRL 的公式对偏度/峰度的口径敏感, 故这里只提供**总体矩**
//! (除以 T), 与 Bailey & López de Prado 的收益分布矩定义一致。样本方差
//! 单独用 `ddof=1`, 与引擎夏普的分母一致。

/// 算术平均。
pub(crate) fn mean(x: &[f64]) -> f64 {
    if x.is_empty() {
        0.0
    } else {
        x.iter().sum::<f64>() / x.len() as f64
    }
}

/// 样本方差(ddof=1)。样本不足 2 或方差非有限时返回 `None`。
pub(crate) fn variance(x: &[f64], mean: f64) -> Option<f64> {
    if x.len() < 2 {
        return None;
    }
    let ss: f64 = x.iter().map(|v| (v - mean) * (v - mean)).sum();
    let var = ss / (x.len() as f64 - 1.0);
    (var.is_finite() && var >= 0.0).then_some(var)
}

/// 总体方差(除以 T), 用于偏度/峰度的标准化。
fn population_variance(x: &[f64], mean: f64) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / x.len() as f64
}

/// 总体偏度 \(\hat\gamma_3 = m_3 / m_2^{3/2}\)。
///
/// 返回 `None` 表示方差为 0 或样本不足 3。
pub(crate) fn skewness(x: &[f64]) -> Option<f64> {
    if x.len() < 3 {
        return None;
    }
    let m = mean(x);
    let m2 = population_variance(x, m);
    if m2 <= 0.0 || !m2.is_finite() {
        return None;
    }
    let m3 = x.iter().map(|v| (v - m).powi(3)).sum::<f64>() / x.len() as f64;
    let out = m3 / m2.powf(1.5);
    out.is_finite().then_some(out)
}

/// 原始峰度 \(\hat\gamma_4 = m_4 / m_2^2\)(正态 = 3, 非超额)。
///
/// PSR 的分母用的是原始峰度; 若误用超额峰度(正态 = 0), 分母会系统性偏小,
/// 显著性被高估。
pub(crate) fn raw_kurtosis(x: &[f64]) -> Option<f64> {
    if x.len() < 4 {
        return None;
    }
    let m = mean(x);
    let m2 = population_variance(x, m);
    if m2 <= 0.0 || !m2.is_finite() {
        return None;
    }
    let m4 = x.iter().map(|v| (v - m).powi(4)).sum::<f64>() / x.len() as f64;
    let out = m4 / (m2 * m2);
    out.is_finite().then_some(out)
}

/// 中位数。输入不必有序; 内部复制并排序。
pub(crate) fn median(x: &[f64]) -> Option<f64> {
    if x.is_empty() {
        return None;
    }
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    Some(quantile_sorted(&v, 0.5))
}

/// 已排序序列的线性插值分位(`q ∈ [0,1]`)。
pub(crate) fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let w = pos - lo as f64;
    sorted[lo] * (1.0 - w) + sorted[hi] * w
}

/// 标准正态 CDF, 用 `libm::erf` 达到双精度精度。
pub(crate) fn normal_cdf(x: f64) -> f64 {
    0.5 * libm::erfc(-x / std::f64::consts::SQRT_2)
}

/// 标准正态分位函数(Acklam 近似 + Halley 一步修正)。
///
/// `p` 必须在 `(0,1)`; 越界返回 `f64::NAN`, 由调用方显式处理。DSR 的
/// `Φ⁻¹(1 - 1/(N·e))` 在 N=1 时为 `Φ⁻¹(1 - 0.3679)`, 远离数值尾部;
/// 但 MinTRL 可能取很小的 α, 故仍做一步修正。
pub(crate) fn normal_quantile(p: f64) -> f64 {
    if !(0.0 < p && p < 1.0) {
        return f64::NAN;
    }
    #[allow(clippy::excessive_precision)]
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_690e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239e0,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838e0,
        -2.549_732_539_343_734e0,
        4.374_664_141_464_968e0,
        2.938_163_982_698_783e0,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996e0,
        3.754_408_661_907_416e0,
    ];
    const P_LOW: f64 = 0.024_25;
    const P_HIGH: f64 = 1.0 - P_LOW;

    let mut x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };

    // Halley 修正: 把近似误差压到接近机器精度。
    let e = normal_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (0.5 * x * x).exp();
    x -= u / (1.0 + 0.5 * x * u);
    x
}

/// FNV-1a 64 位哈希, 用于组合 id/window 指纹。
///
/// 不用 `DefaultHasher`: 它的算法不保证跨 Rust 版本稳定, 而报告要求同一输入
/// 在不同构建上给出同一指纹。FNV-1a 的实现只有几行, 且与字节序无关。
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 把 `f64` 规范化成可哈希的位模式: `0.0` 与 `-0.0` 视为同一个值,
/// 非有限值按位保留(调用方应在此之前拒绝它们)。
pub(crate) fn canonical_bits(x: f64) -> u64 {
    if x == 0.0 {
        0.0f64.to_bits()
    } else {
        x.to_bits()
    }
}

/// 有限值才返回 `Some`, 否则 `None`(JSON 不能表示 NaN/Inf)。
pub(crate) fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn normal_cdf_known_values() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-15);
        assert!((normal_cdf(1.0) - 0.841_344_746_068_543).abs() < 1e-12);
        assert!((normal_cdf(-1.96) - 0.024_997_895_148_220_5).abs() < 1e-12);
    }

    #[test]
    fn normal_quantile_round_trips() {
        for p in [1e-6, 0.001, 0.025, 0.5, 0.975, 0.999, 1.0 - 1e-6] {
            let x = normal_quantile(p);
            let back = normal_cdf(x);
            assert!((back - p).abs() < 1e-10, "p={p} x={x} back={back}");
        }
    }

    #[test]
    fn moments_match_hand_example() {
        // 对称四点: 偏度 0, 总体峰度 m4/m2² = 1.0。
        let x = [-1.0, -1.0, 1.0, 1.0];
        assert!(skewness(&x).unwrap().abs() < 1e-15);
        assert!((raw_kurtosis(&x).unwrap() - 1.0).abs() < 1e-15);
        assert!((mean(&x)).abs() < 1e-15);
        assert!((variance(&x, 0.0).unwrap() - 4.0 / 3.0).abs() < 1e-15);
    }

    #[test]
    fn quantile_interpolates() {
        assert!((quantile_sorted(&[1.0, 2.0, 3.0], 0.25) - 1.5).abs() < 1e-15);
        assert!((quantile_sorted(&[1.0, 2.0, 3.0], 0.75) - 2.5).abs() < 1e-15);
    }
}
