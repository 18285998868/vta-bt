use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkSqzConfig;

/// 收窄分位固定值（下四分位；单参数纪律，分位敏感性留待后续）。
const SQZ_Q_NUM: usize = 1;
const SQZ_Q_DEN: usize = 4;

/// BBRK+收窄过滤：flat + close>upper(下轨对称) **且** 带宽分位通过 → Buy/Sell；
/// 持仓后中线离场（与 BBRK 相同）。带宽=NaN/窗口不足 → 不开仓（P7）。
/// 分位实现：过去N根（含当根）中带宽≤当根的数量 ≤ ceil(N/4) 即通过
/// （≈当根处于下四分位；全平带宽时 cnt=N 不通过，保守）。
pub struct BbrkSqzStrategy {
    pairs: Vec<(f64, usize)>,
}

impl BbrkSqzStrategy {
    pub fn new(config: BbrkSqzConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测)。
pub fn bbrk_sqz_signals(
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    sqz_n: usize,
) -> Vec<Signal> {
    let n = close.len().min(lower.len()).min(mid.len()).min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    // 带宽序列（mid<=0 或非有限 → NaN，即不过滤通过）。
    let mut bw = vec![f64::NAN; n];
    for i in 0..n {
        let (lo, m, up) = (lower[i], mid[i], upper[i]);
        if lo.is_finite() && m.is_finite() && up.is_finite() && m > 0.0 && up > lo {
            bw[i] = (up - lo) / m;
        }
    }
    let allow = (sqz_n * SQZ_Q_NUM).div_ceil(SQZ_Q_DEN).max(1);
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            if ![close[i], lower[i], upper[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            let brk_up = close[i] > upper[i];
            let brk_dn = close[i] < lower[i];
            if !brk_up && !brk_dn {
                continue;
            }
            // 分位门：需要整窗 N 根有限带宽。
            if sqz_n == 0 || i + 1 < sqz_n || !bw[i].is_finite() {
                continue;
            }
            let mut cnt = 0usize;
            let mut ok = true;
            for j in (i + 1 - sqz_n)..=i {
                if !bw[j].is_finite() {
                    ok = false;
                    break;
                }
                if bw[j] <= bw[i] {
                    cnt += 1;
                }
            }
            if !ok || cnt > allow {
                continue;
            }
            if brk_up {
                sig[i] = Signal::Buy;
                pos = 1;
            } else {
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
        } else {
            if ![close[i], mid[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= mid[i] {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(close: &[f64], sigma: f64, sqz_n: usize) -> crate::error::Result<Vec<Signal>> {
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
    Ok(bbrk_sqz_signals(
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        sqz_n,
    ))
}

impl Strategy for BbrkSqzStrategy {
    fn name(&self) -> &'static str {
        "BBRK_SQZ"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, n) = self.pairs[idx];
        format!("sigma={s},sqzn={n}")
    }

    fn generate_signals(
        &self,
        _open: &[f64],
        close: &[f64],
        _volume: &[f64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, n) = self.pairs[param_idx];
        compute(close, s, n)
    }

    fn generate_signals_full(
        &self,
        _open: &[f64],
        _high: &[f64],
        _low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        // 只需 close/BB，双入口逐位一致。
        let (s, n) = self.pairs[param_idx];
        compute(close, s, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    #[test]
    fn test_squeezed_breakout_buys() {
        // 收窄末端突破（带宽递减，当根仍是窗内最低，rank1 ≤ allow1）→ Buy。
        // bw = (up-lo)/mid = [.20,.16,.12,.08,.08]；i=3: 105>104 突破，
        // 窗[0..3]内 bw≤.08 仅当根自己 → cnt=1 ≤ allow(4div突4=1) → Buy。
        let close = vec![100.0, 100.0, 100.0, 105.0, 105.0];
        let lower = vec![90.0, 92.0, 94.0, 96.0, 96.0];
        let mid = vec![100.0; 5];
        let upper = vec![110.0, 108.0, 106.0, 104.0, 104.0];
        let sig = bbrk_sqz_signals(&close, &lower, &mid, &upper, 4);
        assert_eq!(sig[3], Signal::Buy);
    }

    #[test]
    fn test_wide_breakout_blocked() {
        // 带宽扩张中的突破（当根最大）→ 拦住。
        let close = vec![100.0, 100.0, 100.0, 115.0];
        let lower = vec![99.0, 99.0, 99.0, 90.0];
        let mid = vec![100.0; 4];
        let upper = vec![101.0, 101.0, 101.0, 110.0];
        // bw=[.02,.02,.02,.222]；i=3 突破，窗内≤.222 有4个 > 1 → Hold。
        let sig = bbrk_sqz_signals(&close, &lower, &mid, &upper, 4);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_short_window_holds() {
        // 窗口不足 N 根 → 不开仓。
        let close = vec![111.0, 112.0];
        let lower = vec![90.0; 2];
        let mid = vec![100.0; 2];
        let upper = vec![110.0; 2];
        let sig = bbrk_sqz_signals(&close, &lower, &mid, &upper, 4);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkSqzStrategy {
            pairs: vec![(2.5, 50)],
        };
        assert_eq!(s.name(), "BBRK_SQZ");
        assert_eq!(s.param_str(0), "sigma=2.5,sqzn=50");
    }
}
