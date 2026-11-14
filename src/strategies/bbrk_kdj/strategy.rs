use vector_ta::indicators::atr::{AtrInput, AtrParams, atr};
use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};
use vector_ta::indicators::kdj::{KdjInput, KdjParams, kdj};
use vector_ta::indicators::moving_averages::sma::{SmaInput, SmaParams, sma};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkKdjConfig;

/// KC 与视频默认一致：SMA20 ± 1.5·ATR14（固定，不调参）。
const KC_PERIOD: usize = 20;
const KC_MULT: f64 = 1.5;
const ATR_LEN: usize = 14;
/// KDJ：fast_k=25（视频Period 9→25），slow 按库默认（映射假设）。
const KDJ_FAST: usize = 25;
/// “近期内”焊死：止损取触发根前推 STOP_N 根最低/最高。
const STOP_N: usize = 10;

/// BBRK+KDJ逆势波段：布防→触发两段式（见 config 文档）。
/// 状态机：pos（持仓）＋ armed（布防方向 0/±1）＋ exit_armed（见过80/20备战）。
pub struct BbrkKdjStrategy {
    pairs: Vec<(f64, usize)>,
}

impl BbrkKdjStrategy {
    /// 由 `BbrkKdjConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkKdjConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；open/high/low/close + 上下轨 + K + squeeze_on)。
/// 单状态机（pos 0/±1，与 BBRK 同形，不存在双腿持仓冲突）。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn bbrk_kdj_signals(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    lower: &[f64],
    upper: &[f64],
    kvals: &[f64],
    sqz: &[bool],
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(high.len())
        .min(low.len())
        .min(lower.len())
        .min(upper.len())
        .min(kvals.len())
        .min(sqz.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    let mut armed: i8 = 0;
    let mut exit_armed = false;
    let mut stop = f64::NAN;
    for i in 0..n {
        if pos == 0 {
            if ![open[i], close[i], lower[i], upper[i], kvals[i]]
                .iter()
                .all(|v| v.is_finite())
                || !sqz[i]
            {
                // 缺口：不清 armed（布防跨NaN保留，触发时重验）。
                continue;
            }
            let (o, c, k) = (open[i], close[i], kvals[i]);
            // 布防（对侧布防覆盖）。
            if c <= lower[i] && k < 20.0 {
                armed = 1;
            } else if c >= upper[i] && k > 80.0 {
                armed = -1;
            }
            // 触发。
            if armed == 1 && k > 20.0 && c > o {
                sig[i] = Signal::Buy;
                pos = 1;
                armed = 0;
                exit_armed = false;
                let lo = (i + 1).saturating_sub(STOP_N);
                stop = low[lo..=i].iter().fold(f64::INFINITY, |a, &b| a.min(b));
            } else if armed == -1 && k < 80.0 && c < o {
                sig[i] = Signal::Sell;
                pos = -1;
                armed = 0;
                exit_armed = false;
                let lo = (i + 1).saturating_sub(STOP_N);
                stop = high[lo..=i]
                    .iter()
                    .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            }
        } else if pos == 1 {
            if ![close[i], low[i], kvals[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            // 止损优先（保守：同根先看止损）。
            if close[i] <= stop {
                sig[i] = Signal::Close;
                pos = 0;
                continue;
            }
            if kvals[i] > 80.0 {
                exit_armed = true;
            }
            if exit_armed && kvals[i] < 80.0 {
                sig[i] = Signal::Close;
                pos = 0;
            }
        } else {
            if ![close[i], high[i], kvals[i]].iter().all(|v| v.is_finite()) {
                continue;
            }
            if close[i] >= stop {
                sig[i] = Signal::Close;
                pos = 0;
                continue;
            }
            if kvals[i] < 20.0 {
                exit_armed = true;
            }
            if exit_armed && kvals[i] > 20.0 {
                sig[i] = Signal::Close;
                pos = 0;
            }
        }
    }
    sig
}

fn compute(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    bblen: usize,
) -> crate::error::Result<Vec<Signal>> {
    let bb = match bollinger_bands(&BollingerBandsInput::from_slice(
        close,
        BollingerBandsParams {
            period: Some(bblen),
            devup: Some(sigma),
            devdn: Some(sigma),
            matype: Some("sma".to_string()),
            devtype: Some(0),
        },
    )) {
        Ok(v) => v,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let mid_sma = match sma(&SmaInput::from_slice(
        close,
        SmaParams {
            period: Some(KC_PERIOD),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let atr_vals = match atr(&AtrInput::from_slices(
        high,
        low,
        close,
        AtrParams {
            length: Some(ATR_LEN),
        },
    )) {
        Ok(v) => v.values,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let kd = match kdj(&KdjInput::from_slices(
        high,
        low,
        close,
        KdjParams {
            fast_k_period: Some(KDJ_FAST),
            slow_k_period: None,
            slow_k_ma_type: None,
            slow_d_period: None,
            slow_d_ma_type: None,
        },
    )) {
        Ok(v) => v,
        Err(e) => return Err(crate::strategies::param_skipped(e)),
    };
    let n = close.len();
    let mut sqz = vec![false; n];
    for i in 0..n
        .min(bb.upper_band.len())
        .min(mid_sma.len())
        .min(atr_vals.len())
    {
        let (up_b, lo_b, m, a) = (bb.upper_band[i], bb.lower_band[i], mid_sma[i], atr_vals[i]);
        if ![up_b, lo_b, m, a].iter().all(|v| v.is_finite()) || a <= 0.0 {
            continue;
        }
        // 挤压 = BB inside KC（与 TTM 同定义）。
        sqz[i] = up_b < m + KC_MULT * a && lo_b > m - KC_MULT * a;
    }
    // 多空各跑一遍状态机（空头侧把 upper 当 lower 传，约定见 signals）。
    Ok(bbrk_kdj_signals(
        open,
        high,
        low,
        close,
        &bb.lower_band,
        &bb.upper_band,
        &kd.k,
        &sqz,
    ))
}

impl Strategy for BbrkKdjStrategy {
    fn name(&self) -> &'static str {
        "BBRK_KDJ"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        format!("sigma={},bblen={}", self.pairs[idx].0, self.pairs[idx].1)
    }

    fn generate_signals_full(
        &self,
        bars: crate::strategies::Bars<'_>,
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        compute(
            bars.open(),
            bars.high(),
            bars.low(),
            bars.close(),
            self.pairs[param_idx].0,
            self.pairs[param_idx].1,
        )
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    /// 测试夹具: (open, high, low, close, lower, upper, k, sqz) 全长 `n`。
    type Fixture = (
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<bool>,
    );

    fn base(n: usize) -> Fixture {
        // lower=90/upper=110，sqz全开；open=flat100。
        (
            vec![100.0; n],
            vec![105.0; n],
            vec![95.0; n],
            vec![100.0; n],
            vec![90.0; n],
            vec![110.0; n],
            vec![10.0; n],
            vec![true; n],
        )
    }

    #[test]
    fn test_arm_then_trigger_buys() {
        let (o, h, l, mut c, lower, upper, mut k, sqz) = base(3);
        // i=0: c=89<=90 且 K=10<20 → 布防； i=1: K=25>20 且收涨 → Buy。
        c[0] = 89.0;
        k[1] = 25.0;
        c[1] = 101.0;
        let sig = bbrk_kdj_signals(&o, &h, &l, &c, &lower, &upper, &k, &sqz);
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_no_squeeze_no_arm() {
        let (o, h, l, mut c, lower, upper, mut k, _) = base(3);
        c[0] = 89.0;
        k[1] = 25.0;
        c[1] = 101.0;
        let sig = bbrk_kdj_signals(&o, &h, &l, &c, &lower, &upper, &k, &[false; 3]);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_k_never_crosses_no_trigger() {
        let (o, h, l, mut c, lower, upper, k, sqz) = base(3);
        c[0] = 89.0;
        c[1] = 101.0;
        // K恒10：布防了但永不触发。
        let sig = bbrk_kdj_signals(&o, &h, &l, &c, &lower, &upper, &k, &sqz);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_stop_closes_before_flip() {
        let (o, h, l, mut c, lower, upper, mut k, sqz) = base(4);
        c[0] = 89.0;
        k[1] = 25.0;
        c[1] = 101.0; // Buy
        c[2] = 80.0; // 跌破止损（**close** ≤ 止损位 89; 收盘价判定）
        k[2] = 30.0;
        let sig = bbrk_kdj_signals(&o, &h, &l, &c, &lower, &upper, &k, &sqz);
        assert_eq!(sig[1], Signal::Buy);
        assert_eq!(sig[2], Signal::Close);
    }

    #[test]
    fn test_k_flip_exit() {
        let (o, h, mut l, mut c, lower, upper, mut k, sqz) = base(5);
        c[0] = 89.0;
        k[1] = 25.0;
        c[1] = 101.0; // Buy（止损=min(l[0..=1])=95）
        l[2] = 96.0; // 未碰止损
        k[2] = 85.0; // 备战
        k[3] = 75.0; // 翻下 → Close
        c[3] = 102.0;
        let sig = bbrk_kdj_signals(&o, &h, &l, &c, &lower, &upper, &k, &sqz);
        assert_eq!(sig[3], Signal::Close);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkKdjStrategy {
            pairs: vec![(2.5, 20)],
        };
        assert_eq!(s.name(), "BBRK_KDJ");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20");
    }
}
