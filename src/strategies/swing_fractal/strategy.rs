use crate::strategies::{Signal, Strategy, rolling_highest, rolling_lowest};

use super::config::SwingFractalConfig;

/// 摆动点确认标记: `out[i] = true` ⟺ bar `i - right` 被确认为摆动高点。
///
/// 定义(无未来函数, 滞后 `right` 根确认): bar `j` 是摆动高点 ⟺
/// `high[j]` **严格大于**左 `left` 根与右 `right` 根的全部 high, 即
/// `high[j] = max(high[j-left .. j+right])` **且独占**。
///
/// 实现拆成两个互不重叠的窗口, 各用一次 O(n) 滑动极值:
/// - `left_max[j-1] = max(high[j-left ..= j-1])`  (窗 `left`, 取到 `j-1`)
/// - `right_max[i] = max(high[j+1 ..= j+right])`     (窗 `right`, 取到 `i`, `i = j+right`)
///
/// **严格大于**(平台不算摆动点): 与 `../quant-research/pine/indicators/williams_fractals.pine` 一致。
/// 用非严格 `>=` 会在平顶平台上连续确认多个同价摆动点, 使 nH/nL 在同价位反复触发。
///
/// 任一窗口越界(`left_max[j-1]` 为 NaN)时比较为 false, 即**不产生摆动点**(P7, 不静默)。
#[must_use]
fn swing_high_flags(high: &[f64], left: usize, right: usize) -> Vec<bool> {
    let n = high.len();
    let mut out = vec![false; n];
    if left == 0 || right == 0 {
        return out;
    }
    let left_max = rolling_highest(high, left); // 窗口 [j-left+1 ..= j]; 严格左侧窗取 [j-1]
    let right_max = rolling_highest(high, right); // [i-right+1 ..= i] = [j+1 ..= j+right]
    for i in right..n {
        let j = i - right;
        // `left_max[j]` 含 j 自身, 故改用 `left_max[j-1]` 取严格左侧窗。
        // `j == 0` 左侧无窗口 → 不是摆动点, 同时避开 `j - 1` 的 usize 下溢。
        if j == 0 {
            continue;
        }
        out[i] = high[j] > left_max[j - 1] && high[j] > right_max[i];
    }
    out
}

/// [`swing_high_flags`] 的最低点镜像。
#[must_use]
fn swing_low_flags(low: &[f64], left: usize, right: usize) -> Vec<bool> {
    let n = low.len();
    let mut out = vec![false; n];
    if left == 0 || right == 0 {
        return out;
    }
    let left_min = rolling_lowest(low, left);
    let right_min = rolling_lowest(low, right);
    for i in right..n {
        let j = i - right;
        if j == 0 {
            continue;
        }
        out[i] = low[j] < left_min[j - 1] && low[j] < right_min[i];
    }
    out
}

/// 摆动点突破的纯决策函数(可手算单测)。`left` = N, `right` = K。
///
/// 状态机(每根只发一个信号, 与引擎 `Signal` 语义一一对应):
///
/// | 持仓 | 条件 | 信号 | 下一状态 |
/// |---|---|---|---|
/// | flat | `close > nH` | `Buy` | long, `stop = nL` |
/// | flat | `close < nL` | `Sell` | short, `stop = nH` |
/// | long | `close < nL` | `Sell` | short, `stop = nH` (**平多开空**) |
/// | long | `nL <= close < stop` | `Close` | flat (**跟踪止损出局**) |
/// | short | `close > nH` | `Buy` | long, `stop = nL` (**平空开多**) |
/// | short | `stop < close <= nH` | `Close` | flat (**跟踪止损出局**) |
///
/// 持仓期间 `stop = max(stop, nL)`(多头) / `min(stop, nH)`(空头) —— 棘轮, 只朝有利方向。
///
/// **为什么两条出场规则都要**: `stop >= nL` 恒成立, 故 `close < nL` 必然同时击穿止损;
/// 先判 `close < nL`(反手) 再判 `close < stop`(纯止损), 顺序不可交换。
/// 跟踪止损的边际价值在后一条 —— nL 抬高后价格回落但尚未跌破新 nL 时提前锁利。
///
/// **止损为收盘价判定, 非盘中 stop 单**: vta 引擎的成交价只由 `ExecConfig`
/// (次根开盘/收盘) 决定, 无法表达"盘中触及即按止损价成交"。若改用 `low[i] <= stop`
/// 判定, 信号在 bar i 发出而平仓在 bar i+1 开盘, 次日高开即以远优于止损价离场 ——
/// 系统性高估, 与 P4/P12 的延迟口径冲突。故此处统一用 `close`。
///
/// NaN 语义(P7): 摆动点未就绪(`nH` 或 `nL` 任一为 NaN)、输入 NaN 时一律 `Hold`,
/// 不产生任何隐含降级。
#[must_use]
pub fn swing_fractal_signals(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    left: usize,
    right: usize,
) -> Vec<Signal> {
    let n = high.len().min(low.len()).min(close.len());
    let sh = swing_high_flags(high, left, right);
    let sl = swing_low_flags(low, left, right);
    let mut sig = vec![Signal::Hold; n];
    // 最近一次**已确认**的摆动高 / 摆动低(覆盖式更新, 不取最优点)。
    let mut nh = f64::NAN;
    let mut nl = f64::NAN;
    // 多头止损(只上移) / 空头止损(只下移); 反手或出场后按新仓方向重置。
    let mut stop = f64::NAN;
    let mut pos: i8 = 0;

    for i in 0..n {
        // 1) 先确认滞后 `right` 根的摆动点 —— 只用 ≤ i 的信息。
        if sh[i] {
            nh = high[i - right];
        }
        if sl[i] {
            nl = low[i - right];
        }
        // 2) 未就绪 / 输入非法 → Hold, 维持原状态。
        if !(nh.is_finite() && nl.is_finite() && close[i].is_finite()) {
            continue;
        }
        let c = close[i];
        if pos == 0 {
            if c > nh {
                sig[i] = Signal::Buy;
                pos = 1;
                stop = nl;
            } else if c < nl {
                sig[i] = Signal::Sell;
                pos = -1;
                stop = nh;
            }
        } else if pos == 1 {
            stop = stop.max(nl); // 止损跟随 nL 抬高(棘轮, 新的更低 nL 不会拉低它)
            if c < nl {
                sig[i] = Signal::Sell; // 平多开空
                pos = -1;
                stop = nh;
            } else if c < stop {
                sig[i] = Signal::Close; // 跟踪止损出局
                pos = 0;
                stop = f64::NAN;
            }
        } else {
            stop = stop.min(nh);
            if c > nh {
                sig[i] = Signal::Buy; // 平空开多
                pos = 1;
                stop = nl;
            } else if c > stop {
                sig[i] = Signal::Close;
                pos = 0;
                stop = f64::NAN;
            }
        }
    }
    sig
}

/// 摆动点突破(N/K)。空仓突破 `nH` 做多、跌破 `nL` 做空, 止损锚在反向摆动点并
/// 只朝有利方向棘轮; 跌破 `nL` 平多开空、突破 `nH` 平空开多。
pub struct SwingFractalStrategy {
    pairs: Vec<(usize, usize)>,
}

impl SwingFractalStrategy {
    /// 由 `SwingFractalConfig` 构造; 参数网格由该配置展开。
    #[must_use]
    pub fn new(config: SwingFractalConfig) -> Self {
        Self {
            pairs: config.pairs(),
        }
    }
}

impl Strategy for SwingFractalStrategy {
    fn name(&self) -> &'static str {
        "SWINGFRAC"
    }

    fn param_count(&self) -> usize {
        self.pairs.len()
    }

    fn param_str(&self, idx: usize) -> String {
        let (l, r) = self.pairs[idx];
        format!("left={l},right={r}")
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
        let (l, r) = self.pairs[param_idx];
        Ok(swing_fractal_signals(high, low, close, l, r))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    /// 触发路径构造(参数 left=right=1), 逐根可手算:
    ///
    /// ```text
    /// i    :  0    1    2    3    4    5    6    7    8    9
    /// high : 11   13   12   14   13   12   14   13   12   13
    /// low  :  9   11   10   12   11   10   13   12    9   10
    /// close: 11   12   11   15   13   12   14   13   12   9.5
    /// ```
    ///
    /// 摆动高点 j=1→i=2 (nH=13), j=3→i=4 (nH=14), j=6→i=7 (nH=14);
    /// 摆动低点 j=2→i=3 (nL=10), j=5→i=6 (nL=10), j=8→i=9 (**nL 掉回 9**)。
    fn trigger_case() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let high = vec![11.0, 13.0, 12.0, 14.0, 13.0, 12.0, 14.0, 13.0, 12.0, 13.0];
        let low = vec![9.0, 11.0, 10.0, 12.0, 11.0, 10.0, 13.0, 12.0, 9.0, 10.0];
        let close = vec![11.0, 12.0, 11.0, 15.0, 13.0, 12.0, 14.0, 13.0, 12.0, 9.5];
        (high, low, close)
    }

    #[test]
    fn test_swing_high_flags_strict_on_both_windows() {
        // high = 1,2,3,2,1,2, left=right=1: 峰 j=2 于 i=3 确认。
        let high = vec![1.0, 2.0, 3.0, 2.0, 1.0, 2.0];
        assert_eq!(
            swing_high_flags(&high, 1, 1),
            vec![false, false, false, true, false, false]
        );
        // 平台不算: high[j] 与邻居相等 → 非摆动点(严格 >)。
        let flat = vec![1.0, 2.0, 2.0, 1.0];
        assert_eq!(swing_high_flags(&flat, 1, 1), vec![false; 4]);
    }

    #[test]
    fn test_swing_low_flags_strict_on_both_windows() {
        let low = vec![5.0, 4.0, 3.0, 4.0, 5.0, 4.0];
        assert_eq!(
            swing_low_flags(&low, 1, 1),
            vec![false, false, false, true, false, false]
        );
    }

    #[test]
    fn test_warmup_holds_until_both_sides_confirmed() {
        let (high, low, close) = trigger_case();
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        // nH 在 i=2 就绪, nL 到 i=3 才就绪 → 前三根必须 Hold。
        assert_eq!(&sig[..3], &[Signal::Hold; 3]);
        assert_eq!(sig[3], Signal::Buy);
    }

    #[test]
    fn test_trailing_stop_ratchets_up_and_never_down() {
        let (high, low, close) = trigger_case();
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[3], Signal::Buy, "close=15 > nH=13");
        // i=4..8 止损 nL 恒 10, 价格未跌破 → 全程持有。
        for (i, s) in sig.iter().enumerate().take(9).skip(4) {
            assert_eq!(*s, Signal::Hold, "i={i} 应继续持有");
        }
    }

    /// 关键语义: nL 掉回 9 而止损棘轮停在 10, close=9.5 落在 [nL, stop) 区间
    /// → **跟踪止损出局(Close), 不反手**。这是跟踪止损相对纯 nL 破位的增量价值。
    #[test]
    fn test_new_lower_swing_low_does_not_lower_the_stop() {
        let (high, low, close) = trigger_case();
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[9], Signal::Close, "9 <= 9.5 < 10 → 仅平多");
    }

    #[test]
    fn test_break_below_current_swing_low_reverses_to_short() {
        let (high, low, mut close) = trigger_case();
        close[9] = 8.5; // 跌破 nL=9 → 平多开空
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[9], Signal::Sell);
    }

    /// 反手后止损锚在当时的 nH, 并随 nH 下降只下移。
    #[test]
    fn test_short_stop_anchors_at_swing_high_and_follows_down() {
        // 复用触发路径: i=9 平多开空(stop = nH = 14), i=10 出现更低的摆动高 12。
        let high = vec![
            11.0, 13.0, 12.0, 14.0, 13.0, 12.0, 14.0, 13.0, 12.0, 13.0, 12.0,
        ];
        let low = vec![
            9.0, 11.0, 10.0, 12.0, 11.0, 10.0, 13.0, 12.0, 9.0, 10.0, 11.0,
        ];
        // i=10: j=9, high[9]=13 > high[8]=12 ✓ 但 13 > high[10]=12 ✓ → 摆动高 13 于 i=10 确认。
        let close = vec![
            11.0, 12.0, 11.0, 15.0, 13.0, 12.0, 14.0, 13.0, 12.0, 8.5, 13.0,
        ];
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[9], Signal::Sell, "平多开空, 止损锚 nH=14");
        assert_eq!(sig[10], Signal::Hold, "13 <= nH=13, 未反手");
    }

    #[test]
    fn test_short_stop_out_closes_without_reverse() {
        let high = vec![11.0, 13.0, 12.0, 14.0, 13.0, 12.0, 14.0, 13.0, 12.0, 13.0];
        let low = vec![9.0, 11.0, 10.0, 12.0, 11.0, 10.0, 13.0, 12.0, 9.0, 10.0];
        let close = vec![11.0, 12.0, 11.0, 15.0, 13.0, 12.0, 14.0, 13.0, 12.0, 8.5];
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[9], Signal::Sell);
        // 空头止损 = nH = 14; 下一根 close 冲到 13 < 14 → 不动。
        let mut c2 = close.clone();
        c2.push(13.0);
        let mut h2 = high.clone();
        h2.push(12.0);
        let mut l2 = low.clone();
        l2.push(11.0);
        let sig2 = swing_fractal_signals(&h2, &l2, &c2, 1, 1);
        assert_eq!(sig2[10], Signal::Hold);
    }

    #[test]
    fn test_short_break_above_swing_high_flips_long() {
        // i=9 平多开空(stop=nH=14), i=10 收盘 15 > nH=14 → 平空开多。
        let high = vec![
            11.0, 13.0, 12.0, 14.0, 13.0, 12.0, 14.0, 13.0, 12.0, 13.0, 14.0,
        ];
        let low = vec![
            9.0, 11.0, 10.0, 12.0, 11.0, 10.0, 13.0, 12.0, 9.0, 10.0, 11.0,
        ];
        let close = vec![
            11.0, 12.0, 11.0, 15.0, 13.0, 12.0, 14.0, 13.0, 12.0, 8.5, 15.0,
        ];
        let sig = swing_fractal_signals(&high, &low, &close, 1, 1);
        assert_eq!(sig[9], Signal::Sell);
        assert_eq!(sig[10], Signal::Buy, "close=15 > nH=14 → 平空开多");
    }

    #[test]
    fn test_no_signal_when_swing_points_never_form() {
        // 全等高/低 → 严格判定下无摆动点 → 全程 Hold。
        let flat = vec![10.0; 30];
        let sig = swing_fractal_signals(&flat, &flat, &flat, 3, 3);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_nan_input_holds_instead_of_faking_signals() {
        let (high, low, close) = trigger_case();
        let mut nan_close = close.clone();
        nan_close[3] = f64::NAN;
        let sig = swing_fractal_signals(&high, &low, &nan_close, 1, 1);
        assert_eq!(sig[3], Signal::Hold, "close=NaN → 不开仓");
        assert_eq!(sig.iter().filter(|s| **s == Signal::Buy).count(), 0);
    }

    #[test]
    fn test_param_count_and_str() {
        use super::super::config::IntParam;
        let c = SwingFractalConfig {
            left: IntParam::Single(10),
            right: IntParam::Single(5),
        };
        let s = SwingFractalStrategy::new(c);
        assert_eq!(s.name(), "SWINGFRAC");
        assert_eq!(s.param_count(), 1);
        assert_eq!(s.param_str(0), "left=10,right=5");
    }

    /// **P6 双向夹逼**: 两条出场分支必须各自被真实数据触达, 否则其中之一是死代码。
    ///
    /// 夹逼的两个前提由构造保证:
    /// - `trigger_case()` 的 nL 序列为 10 → 10 → 10 → **9**(i=9 处下降):
    ///   止损棘轮停在 10 不降, `close=9.5 ∈ [nL, stop)` → 只有 `Close` 分支能触发。
    /// - `test_new_lower_swing_low_does_not_lower_the_stop` 的对照把 close 改到 8.5 < nL=9:
    ///   `Sell`(平多开空)分支触发, 且此时 `close` 也必然低于 stop=10 —— 即 `Sell`
    ///   分支**不是** `Close` 分支的子集, 二者互斥且都可达。
    #[test]
    fn test_both_exit_branches_are_reachable() {
        let (high, low, mut close) = trigger_case();
        // 前提: 收盘价落在两条线之间 → Close 分支独占。
        assert!(low[8] < close[9] && close[9] < 10.0, "Close 分支前提失效");
        assert_eq!(
            swing_fractal_signals(&high, &low, &close, 1, 1)[9],
            Signal::Close
        );

        // 跌破 nL → Sell 分支独占, 且此处 stop(10) 也被击穿, 证明两分支互斥。
        close[9] = 8.5;
        assert!(close[9] < low[8], "Sell 分支前提失效");
        assert_eq!(
            swing_fractal_signals(&high, &low, &close, 1, 1)[9],
            Signal::Sell
        );
    }
}
