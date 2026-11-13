use vector_ta::indicators::bollinger_bands::{
    BollingerBandsInput, BollingerBandsParams, bollinger_bands,
};

use crate::strategies::{Signal, Strategy};

use super::config::BbrkStrConfig;

/// BBRK+K线强度过滤：flat + close突破 **且** 当根方向一致、实体占比达标
/// （多头 `close>open && |close−open|/(high−low) >= thr`，空头镜像）
/// → Buy/Sell；持仓后中线离场（与 BBRK 相同）。
/// high==low（无影线也无实体）或任一输入 NaN → 不开仓（P7）。
/// 无 high/low 入口用 close 充当 → high==low → 开仓全拦（P7 不退化，
/// 离场中线仍可用，与 BBRK_ADX 口径一致）。
pub struct BbrkStrStrategy {
    pairs: Vec<(f64, usize, f64)>,
}

impl BbrkStrStrategy {
    /// 由 `BbrkStrConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: BbrkStrConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

/// 纯决策函数(可手算单测；7参为OHLC+三轨+阈值的最小完备集)。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn bbrk_str_signals(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    lower: &[f64],
    mid: &[f64],
    upper: &[f64],
    thr: f64,
) -> Vec<Signal> {
    let n = close
        .len()
        .min(open.len())
        .min(high.len())
        .min(low.len())
        .min(lower.len())
        .min(mid.len())
        .min(upper.len());
    let mut sig = vec![Signal::Hold; n];
    let mut pos: i8 = 0;
    for i in 0..n {
        if pos == 0 {
            let (o, h, l, c) = (open[i], high[i], low[i], close[i]);
            if ![o, h, l, c, lower[i], upper[i]]
                .iter()
                .all(|v| v.is_finite())
                || h <= l
            {
                continue;
            }
            let body = (c - o).abs() / (h - l);
            if c > upper[i] && c > o && body >= thr {
                sig[i] = Signal::Buy;
                pos = 1;
            } else if c < lower[i] && c < o && body >= thr {
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

fn compute(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sigma: f64,
    bblen: usize,
    thr: f64,
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
    Ok(bbrk_str_signals(
        open,
        high,
        low,
        close,
        &bb.lower_band,
        &bb.middle_band,
        &bb.upper_band,
        thr,
    ))
}

impl Strategy for BbrkStrStrategy {
    fn name(&self) -> &'static str {
        "BBRK_STR"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (s, b, t) = self.pairs[idx];
        format!("sigma={s},bblen={b},body={t}")
    }

    fn generate_signals_full(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        _volume: &[f64],
        _time: &[i64],
        param_idx: usize,
    ) -> crate::error::Result<Vec<Signal>> {
        let (s, b, t) = self.pairs[param_idx];
        compute(open, high, low, close, s, b, t)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::Strategy;

    fn bands(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (vec![90.0; n], vec![100.0; n], vec![110.0; n])
    }

    #[test]
    fn test_strong_breakout_buys() {
        // 实体占比 (112−100)/(113−99)=12/14≈0.857 ≥ 0.5 → Buy。
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_str_signals(
            &[100.0, 100.0],
            &[101.0, 113.0],
            &[99.0, 99.0],
            &[101.0, 112.0],
            &lower,
            &mid,
            &upper,
            0.5,
        );
        assert_eq!(sig[1], Signal::Buy);
    }

    #[test]
    fn test_weak_breakout_blocked() {
        // 同样突破但长上影：实体 (111−110)/(120−99)=1/21≈0.048 < 0.5 → Hold。
        let (lower, mid, upper) = bands(2);
        let sig = bbrk_str_signals(
            &[111.0, 110.0],
            &[112.0, 120.0],
            &[109.0, 99.0],
            &[111.0, 111.0],
            &lower,
            &mid,
            &upper,
            0.5,
        );
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_wrong_direction_blocked() {
        // 突破上轨但当根收阴 → Hold（方向一致要求）。
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_str_signals(
            &[112.0],
            &[113.0],
            &[109.0],
            &[111.0],
            &lower,
            &mid,
            &upper,
            0.3,
        );
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_flat_bar_holds() {
        // high==low → 不开仓。
        let (lower, mid, upper) = bands(1);
        let sig = bbrk_str_signals(
            &[100.0],
            &[111.0],
            &[111.0],
            &[111.0],
            &lower,
            &mid,
            &upper,
            0.3,
        );
        assert_eq!(sig[0], Signal::Hold);
    }

    #[test]
    fn test_name_and_params() {
        let s = BbrkStrStrategy {
            pairs: vec![(2.5, 20, 0.5)],
        };
        assert_eq!(s.name(), "BBRK_STR");
        assert_eq!(s.param_str(0), "sigma=2.5,bblen=20,body=0.5");
    }
}
