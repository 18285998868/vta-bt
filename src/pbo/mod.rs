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

mod cscv;
mod report;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use cscv::cscv;
pub use report::run_pbo_report;
