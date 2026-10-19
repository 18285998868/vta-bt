use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy, money_flow_index};

use super::config::BbrkM2Config;

/// MFI 周期（官方规则：取带宽周期之半；BB20 → 10，预注册固定）。
pub const M2_MFI_N: usize = 10;
/// %b 多空阈值（官方：多头 0.8/80；空头镜像 0.2/20，见设计注）。
pub const M2_PCTB_LONG: f64 = 0.8;
pub const M2_PCTB_SHORT: f64 = 0.2;
pub const M2_MFI_LONG: f64 = 80.0;
pub const M2_MFI_SHORT: f64 = 20.0;

/// Bollinger Method II（官方手册）：
/// flat + %b>0.8 且 MFI(10)>80 → Buy（强价格经强指标确认）；
/// flat + %b<0.2 且 MFI(10)<20 → Sell（镜像假设：原文详写买侧，
/// “price weakness confirmed by weak indicators generates sell”）；
/// 持仓后中线离场（原文为修正抛物线/对侧带触碰；为隔离“进场定义”
/// 差异、与 BBRK 家族可比，此处统一中线——对照的是进场规则）。
/// %b/MFI 任一 NaN → 不开仓（P7）。
/// `generate_signals`（无 H/L 量能三要素不齐）恒 Hold（P7，不静默降级）；
/// 生产入口为 `generate_signals_full`（run_combo 只调 full）。
pub struct BbrkM2Strategy {
    sigmas: Vec<f64>,
}

impl BbrkM2Strategy {
    /// 由 [`BbrkM2Config`] 构造; 参数网格由该配置展开。
    pub fn new(config: BbrkM2Config) -> Self {
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

/// 纯决策函数(可手算单测；mfi 由调用方按官方口径供给）。
pub fn bbrk_m2_signals(
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
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            let b = pct_b(close[i], lower[i], upper[i]);
            let f = mfi[i];
            if !b.is_finite() || !f.is_finite() || !mid[i].is_finite() {
                continue;
            }
            if b > M2_PCTB_LONG && f > M2_MFI_LONG {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if b < M2_PCTB_SHORT && f < M2_MFI_SHORT {
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

impl Strategy for BbrkM2Strategy {
    fn name(&self) -> &'static str {
        "BBRK_M2"
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

impl BbrkM2Strategy {
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
        let mfi = money_flow_index(high, low, close, volume, M2_MFI_N);
        Ok(bbrk_m2_signals(
            close,
            &bb.lower_band,
            &bb.middle_band,
            &bb.upper_band,
            &mfi,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_strong_confirmed_buys() {
        // %b=(108−90)/20=0.9>0.8 且 MFI 85>80 → Buy。
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_m2_signals(&[108.0], &lower, &mid, &upper, &[85.0]);
        assert_eq!(sig[0], Signal::Buy);
    }

    #[test]
    fn test_unconfirmed_strength_blocked() {
        // 价格强（%b=0.9）但 MFI 弱（70）→ Hold（确认缺席不开仓）。
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_m2_signals(&[108.0], &lower, &mid, &upper, &[70.0]);
        assert_eq!(sig[0], Signal::Hold);
        // 指标强但价格不够强（%b=0.5）→ Hold。
        let sig2 = bbrk_m2_signals(&[100.0], &lower, &mid, &upper, &[85.0]);
        assert_eq!(sig2[0], Signal::Hold);
    }

    #[test]
    fn test_weak_confirmed_sells() {
        // %b=(92−90)/20=0.1<0.2 且 MFI 15<20 → Sell。
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_m2_signals(&[92.0], &lower, &mid, &upper, &[15.0]);
        assert_eq!(sig[0], Signal::Sell);
    }

    #[test]
    fn test_threshold_is_strict() {
        // 等于阈值不触发（严格大于/小于）。
        let (lower, mid, upper) = bands(1);
        // %b=0.8 → (106−90)/20=0.8；MFI=80：双等于 → Hold。
        let sig = bbrk_m2_signals(&[106.0], &lower, &mid, &upper, &[80.0]);
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_exit_same_as_bbrk() {
        let (lower, mid, upper) = bands(3);
        let sig = bbrk_m2_signals(&[108.0, 105.0, 100.0], &lower, &mid, &upper, &[85.0; 3]);
        assert_eq!(sig[0], Signal::Buy);
        assert_eq!(sig[1], Signal::Hold);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_nan_hold() {
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_m2_signals(&[108.0], &lower, &mid, &upper, &[f64::NAN]);
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkM2Strategy { sigmas: vec![2.0] };
        assert_eq!(s.name(), "BBRK_M2");
        assert_eq!(s.param_str(0), "sigma=2");
        assert_eq!((M2_MFI_N, M2_PCTB_LONG, M2_MFI_LONG), (10, 0.8, 80.0));
    }
}
