use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy, money_flow_index};

use super::config::BbrkM3Config;

/// 形态回看窗（预注册固定 10 根，不调参，防 P2）。
pub const M3_W: usize = 10;
/// MFI 确认周期（官方规则：带宽周期之半；BB20 → 10）。
pub const M3_MFI_N: usize = 10;

/// Bollinger Method III 原文口径（S&C 1992 +《Bollinger on Bollinger Bands》）：
/// W 底 ⇔ 回看窗内先一根 %b<0（第一低，轨外）、后一根 %b∈[0,1]（第二低，
/// 落回带内——原文不要求第二低与第一低的价格高低，只看相对带的位置），
/// 且 MFI 在第二处不创新低（MFI[b]>MFI[a]，原文 RSI/MFI 确认）；确认根
/// `close>mid`（吃上半段）进多头。M 顶镜像（先 %b>1、后 %b∈[0,1]、
/// MFI[b]<MFI[a]、`close<mid` 进空）。
/// 离场与 BBRK 同形（中线），以隔离“进场定义”差异。
/// %b/MFI 任一 NaN → 不开仓（P7）。
/// 取窗内“最近一对”（a=第二处之前最近的轨外 tag，b=最近的带内 tag）；
/// 摆动点结构检测不做（原文只约束带相对位置）。
/// `generate_signals`（无 H/L）恒 Hold（P7）；生产入口为 full。
pub struct BbrkM3Strategy {
    sigmas: Vec<f64>,
}

impl BbrkM3Strategy {
    /// 由 [`BbrkM3Config`] 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkM3Config) -> Self {
        Self {
            sigmas: config.values(),
        }
    }
}

fn pct_b(close: f64, lower: f64, upper: f64) -> f64 {
    if !close.is_finite() || !lower.is_finite() || !upper.is_finite() {
        return f64::NAN;
    }
    let w = upper - lower;
    if !w.is_finite() || w <= 0.0 {
        return f64::NAN;
    }
    (close - lower) / w
}

/// 窗 `[lo, i)` 内：b=最近的带内 tag（`inside`），a=b 之前最近的轨外 tag
/// （`outside`）。mfi[a]/mfi[b] 须有限（调用方保证向量等长；NaN→None）。
fn last_pair(
    pct: &[f64],
    mfi: &[f64],
    lo: usize,
    i: usize,
    outside: impl Fn(f64) -> bool,
    inside: impl Fn(f64) -> bool,
) -> Option<(usize, usize)> {
    let mut b: Option<usize> = None;
    for j in (lo..i).rev() {
        let v = pct[j];
        if !v.is_finite() || !mfi[j].is_finite() {
            continue;
        }
        if inside(v) {
            b = Some(j);
            break;
        }
    }
    let b = b?;
    let mut a: Option<usize> = None;
    for j in (lo..b).rev() {
        let v = pct[j];
        if !v.is_finite() || !mfi[j].is_finite() {
            continue;
        }
        if outside(v) {
            a = Some(j);
            break;
        }
    }
    a.map(|a| (a, b))
}

/// 纯决策函数(可手算单测；mfi 由调用方按官方口径供给）。
#[must_use]
pub fn bbrk_m3_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    mfi: &[f64],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(lower.len())
        .min(mid.len())
        .min(upper.len())
        .min(mfi.len());
    let pct: Vec<f64> = (0..n)
        .map(|k| pct_b(close[k], lower[k], upper[k]))
        .collect();
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], mid[i], upper[i]]
                .iter()
                .all(|v| v.is_finite())
            {
                continue;
            }
            let lo = i.saturating_sub(M3_W);
            // W 底：先轨外 %b<0、后带内 0<=%b<=1，且 MFI 不创新低。
            let w_ok = last_pair(&pct, mfi, lo, i, |v| v < 0.0, |v| (0.0..=1.0).contains(&v))
                .is_some_and(|(a, b)| mfi[b] > mfi[a])
                && close[i] > mid[i];
            // M 顶：先轨外 %b>1、后带内 0<=%b<=1，且 MFI 不创新高。
            let m_ok = last_pair(&pct, mfi, lo, i, |v| v > 1.0, |v| (0.0..=1.0).contains(&v))
                .is_some_and(|(a, b)| mfi[b] < mfi[a])
                && close[i] < mid[i];
            if w_ok {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if m_ok {
                sig[i] = Signal::Sell;
                pos = -1;
            }
        } else if pos == 1 {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] <= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else if close[i] >= mid[i] {
            sig[i] = Signal::Close;
            pos = 0;
        }
    }
    sig
}

impl Strategy for BbrkM3Strategy {
    fn name(&self) -> &'static str {
        "BBRK_M3"
    }

    fn param_count(&self) -> usize {
        self.sigmas.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("sigma={}", self.sigmas[idx])
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        self.compute(high, low, close, volume, self.sigmas[param_idx])
    }
}

impl BbrkM3Strategy {
    fn compute(
        &self,
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
        sigma: f64,
    ) -> crate::error::Result<Vec<Signal>> {
        let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
            close,
            BollingerBandsParams {
                period: Some(20),
                devup: Some(sigma),
                devdn: Some(sigma),
                matype: Some("sma".to_string()),
                devtype: Some(0),
            },
        )) {
            Ok(v) => v,
            Err(_) => return Ok(vec![Signal::Hold; close.len()]),
        };
        let mfi = money_flow_index(high, low, close, volume, M3_MFI_N);
        Ok(bbrk_m3_signals(
            close,
            &bb.lower_band,
            &bb.middle_band,
            &bb.upper_band,
            &mfi,
        ))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    // 常数带 lower=90/mid=100/upper=110（宽20）：close<90 ⇔ %b<0，
    // 90<=close<=110 ⇔ %b∈[0,1]，close>110 ⇔ %b>1。
    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_w_outside_then_inside_buys() {
        // j0:88(%b<0,MFI 30) → j2:91(%b∈带内,MFI 40确认) → j4:101>mid Buy。
        let (lower, mid, upper) = bands(5);
        let close = vec![88.0, 95.0, 91.0, 96.0, 101.0];
        let mfi = vec![30.0, 35.0, 40.0, 42.0, 45.0];
        let sig = bbrk_m3_signals(&close, &lower, &mid, &upper, &mfi);
        assert_eq!(sig[4], Signal::Buy);
        assert!(sig[..4].iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_w_second_still_outside_blocked() {
        // 窗内无“落回带内”的第二处（j0–j3 全在轨外）→ Hold。
        // 这正是 v1（bbrk_orth）会进而原文不进的情形。
        let (lower, mid, upper) = bands(5);
        let close = vec![88.0, 89.0, 87.0, 86.0, 101.0];
        let mfi = vec![30.0, 35.0, 40.0, 42.0, 45.0];
        let sig = bbrk_m3_signals(&close, &lower, &mid, &upper, &mfi);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_w_indicator_not_confirmed_blocked() {
        // MFI 第二处创新低（25<30）→ 确认缺席 → Hold。
        let (lower, mid, upper) = bands(5);
        let close = vec![88.0, 95.0, 91.0, 96.0, 101.0];
        let mfi = vec![30.0, 28.0, 25.0, 26.0, 27.0];
        let sig = bbrk_m3_signals(&close, &lower, &mid, &upper, &mfi);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_m_top_mirror_sells() {
        // j0:112(%b>1,MFI 70) → j2:109(%b∈带内,MFI 60确认) → j4:99<mid Sell。
        let (lower, mid, upper) = bands(5);
        let close = vec![112.0, 105.0, 109.0, 104.0, 99.0];
        let mfi = vec![70.0, 68.0, 60.0, 58.0, 55.0];
        let sig = bbrk_m3_signals(&close, &lower, &mid, &upper, &mfi);
        assert_eq!(sig[4], Signal::Sell);
    }

    #[test]
    fn test_exit_same_as_bbrk() {
        let (lower, mid, upper) = bands(6);
        let close = vec![88.0, 95.0, 91.0, 96.0, 101.0, 100.0];
        let mfi = vec![30.0, 35.0, 40.0, 42.0, 45.0, 44.0];
        let sig = bbrk_m3_signals(&close, &lower, &mid, &upper, &mfi);
        assert_eq!(sig[4], Signal::Buy);
        assert_eq!(sig[5], Signal::Close);
    }

    #[test]
    fn test_nan_hold() {
        let nan = vec![f64::NAN, f64::NAN];
        let sig = bbrk_m3_signals(&nan, &nan, &nan, &nan, &nan);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkM3Strategy { sigmas: vec![2.0] };
        assert_eq!(s.name(), "BBRK_M3");
        assert_eq!(s.param_str(0), "sigma=2");
        assert_eq!((M3_W, M3_MFI_N), (10, 10));
    }
}
