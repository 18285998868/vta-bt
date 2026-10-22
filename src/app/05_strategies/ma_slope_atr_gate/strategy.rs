use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::strategies::{Signal, Strategy};

use super::config::{AtrKind, MaSlopeAtrGateConfig, Params};

/// 均线斜率 / ATR 波动尺度 门控策略。
///
/// 因子(只用 t 时刻及以前数据):
/// ```text
/// slope_t = ma_t / ma_{t-k} - 1              # 均线相对变化率
/// f_t     = slope_t / (atr_t / close_t)     # 趋势强度 / 波动尺度(无量纲)
/// z_t     = (f_t - rolling_mean(f, z_win)) / rolling_std(f, z_win)
/// ```
///
/// 仓位(只做多, 不持空): `close > ma && z > th` 持多, 否则空仓。信号以 `Buy` /
/// `Close` 表达 —— 引擎把 `Close` 映射为 `Side::Flat`, 故空仓是原生状态而非"持空"。
///
/// 分母用波动尺度而非价格水平: 波动率相对自身长期水平抬升时因子被自动压缩,
/// 波动收缩期被放大 —— 等价于一个隐式的波动状态调节器。
pub struct MaSlopeAtrGateStrategy {
    quints: Vec<Params>,
}

impl MaSlopeAtrGateStrategy {
    /// 由 [`MaSlopeAtrGateConfig`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: MaSlopeAtrGateConfig) -> Self {
        Self {
            quints: config.quints(),
        }
    }
}

/// 滚动均值与样本标准差。
///
/// 与 pandas `rolling().std()` 对齐: **样本标准差(ddof=1)**, 窗口不足时前 `n-1` 项为
/// NaN; 窗口含 NaN 时整窗为 NaN, 传播到信号层即保持空仓。
///
/// 用两遍法而非 `sum(x^2) - n·mean^2`: 因子的量级可达 ~20, 两项相减会因**灾难性
/// 抵消**把方差抹成 0(曾导致门控永不开仓)。复杂度 `O(len·n)`, 对本策略的日线规模
/// (数千根 × 数十窗口)完全够用, 换来数值可靠。
#[must_use]
fn rolling_mean_std(data: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let len = data.len();
    let mut mean = vec![f64::NAN; len];
    let mut std = vec![f64::NAN; len];
    if n < 2 || len < n {
        return (mean, std);
    }
    let count = n as f64;
    for i in (n - 1)..len {
        let win = &data[i + 1 - n..=i];
        let m = win.iter().sum::<f64>() / count;
        let var = win.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (count - 1.0);
        mean[i] = m;
        std[i] = var.sqrt();
    }
    (mean, std)
}

/// 真实波幅序列: `TR = max(high-low, |high-prev_close|, |low-prev_close|)`。
#[must_use]
pub fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut tr = vec![f64::NAN; n];
    if n == 0 || high.len() != n || low.len() != n {
        return tr;
    }
    tr[0] = high[0] - low[0];
    for i in 1..n {
        let prev = close[i - 1];
        tr[i] = (high[i] - low[i])
            .max((high[i] - prev).abs())
            .max((low[i] - prev).abs());
    }
    tr
}

/// **SMA 平滑**的真实波幅均值(简单移动平均)。
///
/// 与 `vector_ta::atr` 的 Wilder RMA 不同: 此处是等权滑动平均, 因子研究侧
/// (`kline-reader` 配套脚本)用的就是这一口径, 两者必须对齐才能复现研究结论。
/// 简单移动平均的真实波幅均值(SMA 口径)。
///
/// # Errors
/// `n == 0` 或数据短于窗口时返回 [`crate::error::AppError`](crate::error::AppError);
/// 调用方必须把它转成 `ParamSkipped`, 而不是静默全 Hold。
pub fn sma_atr(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    n: usize,
) -> crate::error::Result<Vec<f64>> {
    let tr = true_range(high, low, close);
    if n == 0 {
        return Err(crate::strategies::param_skipped(
            "MA_SLOPE_ATR_GATE: atr_n 必须 >= 1",
        ));
    }
    if tr.len() < n {
        return Err(crate::strategies::param_skipped(format!(
            "MA_SLOPE_ATR_GATE: ATR 窗口 {n} > 数据长度 {}",
            tr.len()
        )));
    }
    let mut out = vec![f64::NAN; tr.len()];
    let mut sum = 0.0;
    for (i, v) in tr.iter().enumerate() {
        sum += v;
        if i >= n {
            sum -= tr[i - n];
        }
        if i + 1 >= n {
            out[i] = sum / n as f64;
        }
    }
    Ok(out)
}

/// Wilder 平滑的真实波幅均值(取自 `vector_ta`, 为平台通用定义)。
///
/// # Errors
/// ATR 不可计算(窗口 > 数据长度等)时返回 [`crate::error::AppError`]。
pub fn wilder_atr(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    n: usize,
) -> crate::error::Result<Vec<f64>> {
    match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams { length: Some(n) },
    )) {
        Ok(v) => Ok(v.values),
        Err(e) => Err(crate::strategies::param_skipped(e)),
    }
}

/// 按 `kind` 选平滑方式。
///
/// **无静默兜底**: `kind` 是枚举, 非法取值在反序列化期就不可表示;
/// 原实现收 `&str` 且写了 `_ => sma_atr` —— 拼错的取值会**静默**变成 SMA,
/// 报告里的口径与配置声明不一致, 且无任何提示。
///
/// # Errors
/// 透传 [`sma_atr`] / [`wilder_atr`] 的错误。
pub fn atr_by_kind(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    n: usize,
    kind: AtrKind,
) -> crate::error::Result<Vec<f64>> {
    match kind {
        AtrKind::Sma => sma_atr(high, low, close, n),
        AtrKind::Wilder => wilder_atr(high, low, close, n),
    }
}

/// 标准差的**近零阈值**。
///
/// 为什么不能只用 `std == 0.0`: 浮点下常量序列的样本标准差**不会**精确为 0
/// (两遍法也一样), 实测落在 1e-16 量级。原实现用 `std[i] == 0.0` 当除零护栏,
/// 这条分支实际**几乎永不触发** —— 于是 `z = (factor - mean) / std` 在 std 近零时
/// 被放大成天文数字, 门控被**静默打开**, 产生本不该有的 Buy 信号。
/// 这是"用浮点相等做数值护栏"的典型失效: 护栏在, 但形同虚设。
const STD_EPS: f64 = 1e-12;

/// 由均线与 ATR 序列生成门控信号。公开以便测试与复用。
///
/// 长度不一致、窗口非法或输入含非有限值时返回全 `Close`(空仓), 不静默产出假信号。
#[must_use]
pub fn gate_signals(
    close: &[f64],
    ma: &[f64],
    atr: &[f64],
    slope_k: usize,
    z_win: usize,
    th: f64,
) -> Vec<Signal> {
    let n = close.len();
    let mut sig = vec![Signal::Close; n]; // 默认空仓: 条件不满足即不在场
    if n == 0 || ma.len() != n || atr.len() != n || slope_k == 0 || z_win == 0 {
        return sig;
    }

    let mut factor = vec![f64::NAN; n];
    for i in 0..n {
        let (c, &m, &a) = (close[i], &ma[i], &atr[i]);
        if !c.is_finite() || c <= 0.0 || !m.is_finite() || !a.is_finite() || a <= 0.0 {
            continue;
        }
        if i < slope_k {
            continue;
        }
        let prev = ma[i - slope_k];
        if !prev.is_finite() || prev <= 0.0 {
            continue;
        }
        let scale = a / c;
        if scale > 0.0 {
            factor[i] = (m / prev - 1.0) / scale;
        }
    }

    let (mean, std) = rolling_mean_std(&factor, z_win);
    for i in 0..n {
        // 近零标准差 → 保持空仓(见 `STD_EPS` 的说明: 不能用 `== 0.0`)。
        if factor[i].is_nan() || mean[i].is_nan() || !std[i].is_finite() || std[i].abs() < STD_EPS {
            continue;
        }
        let z = (factor[i] - mean[i]) / std[i];
        sig[i] = if close[i] > ma[i] && z > th {
            Signal::Buy
        } else {
            Signal::Close
        };
    }
    sig
}

impl Strategy for MaSlopeAtrGateStrategy {
    fn name(&self) -> &'static str {
        "MA_SLOPE_ATR_GATE"
    }

    fn param_count(&self) -> usize {
        self.quints.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (ma, k, a, z, t, kind) = self.quints[idx];
        format!("ma{ma}_k{k}_atr{a}_{kind}_z{z}_th{t}")
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        // `Params` 是 `Copy` 的(不含 `String`), 故此处**不克隆**任何堆数据。
        let (ma_len, slope_k, atr_n, z_win, th, atr_kind) = self.quints[param_idx];
        if ma_len == 0 || slope_k == 0 || atr_n == 0 || z_win == 0 {
            return Err(crate::strategies::param_skipped(format!(
                "MA_SLOPE_ATR_GATE: 参数含 0 (ma={ma_len}, k={slope_k}, atr={atr_n}, z={z_win})"
            )));
        }

        let ma = match sma(&SmaInput::from_slice(
            close,
            SmaParams {
                period: Some(ma_len),
            },
        )) {
            Ok(v) => v.values,
            Err(e) => return Err(crate::strategies::param_skipped(e)),
        };
        let atr_values = atr_by_kind(high, low, close, atr_n, atr_kind)?;

        Ok(gate_signals(close, &ma, &atr_values, slope_k, z_win, th))
    }
}

/// 确定性伪随机游走: `drift` 为每根期望涨幅, `vol` 为波动幅度。
/// 用固定 LCG 而非 rand, 保证测试可复现。
#[cfg(test)]
fn random_walk(n: usize, drift: f64, vol: f64, seed0: u64) -> Vec<f64> {
    let mut seed = seed0;
    let mut price = 100.0;
    (0..n)
        .map(|_| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = (seed >> 33) as f64 / (1u64 << 31) as f64; // [0, 1)
            let u = unit * 2.0 - 1.0; // [-1, 1)
            price *= 1.0 + drift + vol * u;
            price
        })
        .collect()
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::config::{F64Param, PeriodParam};

    use super::*;

    fn config() -> MaSlopeAtrGateConfig {
        MaSlopeAtrGateConfig {
            ma: PeriodParam::Single(100),
            slope_k: PeriodParam::Single(20),
            atr_n: PeriodParam::Single(14),
            z_win: PeriodParam::Single(60),
            th: F64Param::Single(0.5),
            atr_kind: AtrKind::Sma,
        }
    }

    #[test]
    fn rolling_mean_std_matches_sample_std() {
        let d = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (mean, std) = rolling_mean_std(&d, 5);
        assert!((mean[4] - 3.0).abs() < 1e-12);
        // ddof=1 -> sqrt(10/4)
        assert!((std[4] - (10.0_f64 / 4.0).sqrt()).abs() < 1e-12);
        assert!(mean[3].is_nan() && std[3].is_nan());
    }

    #[test]
    fn constant_series_yields_near_zero_std() {
        // 常量序列在浮点下不会得到精确 0 的标准差(两遍法也是), 故此处只锁定
        // 「近零」这一事实; 真正的近零标准差由 `gate_signals` 内的 `STD_EPS`
        // 分支兜底为保持空仓。
        let n: usize = 300;
        let close: Vec<f64> = (0..n).map(|i| 100.0 * 1.005f64.powi(i as i32)).collect();
        let ma: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let atr: Vec<f64> = close.iter().map(|c| c * 0.02).collect();
        let mut factor = vec![f64::NAN; n];
        for i in 20..n {
            factor[i] = (ma[i] / ma[i - 20] - 1.0) / (atr[i] / close[i]);
        }
        let (_, std) = rolling_mean_std(&factor, 60);
        assert!(std[299] < 1e-9, "常量序列的标准差应近零, 实际 {}", std[299]);
    }

    /// 回归: 近零标准差**必须**让门控保持空仓, 且该用例**无条件**执行。
    ///
    /// 可触发场景(两个缺陷叠加):
    /// 1. 生产代码用 `std[i] == 0.0` 当除零护栏 —— 浮点下常量序列的标准差是
    ///    1e-16 量级而非精确 0, 故该分支**几乎永不触发**; std 近零时
    ///    `z = (factor - mean) / std` 被放大成天文数字, 门控被静默打开。
    /// 2. 配套单测写成 `if std[119] == 0.0 { ... }` —— 条件不成立时整个断言
    ///    **被跳过**, 缺陷 1 因此永远测不出来。
    ///
    /// 现在: 护栏用容差 `STD_EPS`, 单测无条件断言。
    #[test]
    fn near_zero_std_keeps_flat_unconditionally() {
        // 直接构造 std 恰为 0 的窗口: 门控不得除零, 应保持空仓。
        let n: usize = 120;
        let close = vec![100.0; n];
        let ma = vec![99.0; n]; // close > ma 恒成立, 只剩 z 门槛把关
        let atr = vec![2.0; n];
        let factor = vec![5.0; n]; // 常量 => std 窗口内为 0 或近零
        let (_, std) = rolling_mean_std(&factor, 60);
        // 前提: 该窗口的标准差确实落在"近零"区间(否则本用例测不到目标分支)。
        assert!(
            std[119].abs() < STD_EPS,
            "常量窗口的标准差应近零, 实际 {}",
            std[119]
        );
        // 无条件断言: 不得写成 `if ... { }` 把断言包起来。
        let sig = gate_signals(&close, &ma, &atr, 20, 60, 0.5);
        assert!(
            !sig.contains(&Signal::Buy),
            "近零标准差下不得开仓(否则 z 被放大成假信号)"
        );
    }

    #[test]
    fn warmup_is_flat_not_long() {
        let close = random_walk(300, 0.001, 0.02, 7);
        let ma = close.clone();
        let atr = vec![2.0; 300];
        let sig = gate_signals(&close, &ma, &atr, 20, 60, 0.5);
        // 预热期(slope_k + z_win = 80 之前)不得出现 Buy
        assert!(sig[..80].iter().all(|s| *s == Signal::Close));
    }

    #[test]
    fn uptrend_enters_long() {
        let close = random_walk(400, 0.002, 0.015, 42);
        let high: Vec<f64> = close.iter().map(|c| c * 1.01).collect();
        let low: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let atr = vec![0.0; 400]; // 占位, 下方直接用 gate_signals 传入真实 ATR
        let ma: Vec<f64> = {
            let mut sum = 0.0;
            close
                .iter()
                .enumerate()
                .map(|(i, &c)| {
                    sum += c;
                    if i >= 100 {
                        sum -= close[i - 100];
                    }
                    if i + 1 >= 100 { sum / 100.0 } else { f64::NAN }
                })
                .collect()
        };
        let _ = (high, low, atr);
        let true_range: Vec<f64> = close
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                if i == 0 {
                    0.0
                } else {
                    (c - close[i - 1]).abs()
                }
            })
            .collect();
        let mut atr120 = vec![f64::NAN; 400];
        let mut s = 0.0;
        for (i, v) in true_range.iter().enumerate() {
            s += v;
            if i >= 14 {
                s -= true_range[i - 14];
            }
            if i + 1 >= 14 {
                atr120[i] = s / 14.0;
            }
        }
        let sig = gate_signals(&close, &ma, &atr120, 20, 60, 0.5);
        let above: usize = (0..400)
            .filter(|&i| !ma[i].is_nan() && close[i] > ma[i])
            .count();
        let buys = sig.iter().filter(|s| **s == Signal::Buy).count();
        assert!(
            buys > 0,
            "带漂移的随机游走中应出现做多信号; close>ma {above} 根, Buy {buys} 根, 末价 {:.2}",
            close[399]
        );
    }

    #[test]
    fn sma_atr_matches_manual_mean() {
        let high = vec![10.0, 11.0, 12.0, 13.0];
        let low = vec![9.0, 10.0, 11.0, 12.0];
        let close = vec![9.5, 10.5, 11.5, 12.5];
        let out = sma_atr(&high, &low, &close, 2).expect("数据足够");
        let tr = true_range(&high, &low, &close);
        assert!((out[1] - (tr[0] + tr[1]) / 2.0).abs() < 1e-12);
        assert!((out[3] - (tr[2] + tr[3]) / 2.0).abs() < 1e-12);
        assert!(out[0].is_nan());
    }

    #[test]
    fn sma_and_wilder_atr_differ_but_both_finite() {
        let n = 300;
        let close = random_walk(n, 0.001, 0.02, 11);
        let high: Vec<f64> = close.iter().map(|c| c * 1.01).collect();
        let low: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let a = sma_atr(&high, &low, &close, 20).expect("数据足够");
        let b = wilder_atr(&high, &low, &close, 20).expect("数据足够");
        assert!(a[250].is_finite() && b[250].is_finite());
        // 平滑方式不同 -> 数值必然不同, 这是两套口径的分歧点
        assert!(
            (a[250] - b[250]).abs() > 1e-9,
            "SMA 与 Wilder 应给出不同结果"
        );
        assert_eq!(
            atr_by_kind(&high, &low, &close, 20, AtrKind::Sma).expect("数据足够")[250],
            a[250]
        );
        assert_eq!(
            atr_by_kind(&high, &low, &close, 20, AtrKind::Wilder).expect("数据足够")[250],
            b[250]
        );
    }

    #[test]
    fn length_mismatch_returns_all_close() {
        let sig = gate_signals(&[1.0, 2.0], &[1.0], &[1.0, 2.0], 1, 2, 0.5);
        assert_eq!(sig, vec![Signal::Close; 2]);
    }

    #[test]
    fn strategy_generates_signals() {
        let close = random_walk(400, 0.002, 0.015, 42);
        let high: Vec<f64> = close.iter().map(|c| c * 1.01).collect();
        let low: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let s = MaSlopeAtrGateStrategy::new(config());
        assert_eq!(s.param_count(), 1);
        assert_eq!(s.name(), "MA_SLOPE_ATR_GATE");
        assert_eq!(s.param_str(0), "ma100_k20_atr14_sma_z60_th0.5");
        let sig = s
            .generate_signals_full(&[], &high, &low, &close, &[], &[], 0)
            .unwrap();
        assert_eq!(sig.len(), close.len());
        assert!(sig.contains(&Signal::Buy));
    }

    /// **契约**: MA / ATR 算不出来时必须 `ParamSkipped`, 不得返回全 Close
    /// (全 Close 在报告里与"策略主动空仓"不可区分)。
    #[test]
    fn short_series_is_param_skipped() {
        let close = vec![100.0; 5];
        let s = MaSlopeAtrGateStrategy {
            quints: vec![(20, 5, 14, 20, 0.5, AtrKind::Sma)],
        };
        let err = s
            .generate_signals_full(&[], &close, &close, &close, &[], &[], 0)
            .expect_err("MA(20)/ATR(14) 在 5 根 bar 上不可计算");
        assert!(
            matches!(err, crate::error::AppError::ParamSkipped { .. }),
            "必须是 ParamSkipped, 实际: {err}"
        );
        assert!(sma_atr(&close, &close, &close, 14).is_err());
        assert!(wilder_atr(&close, &close, &close, 14).is_err());
    }
}
