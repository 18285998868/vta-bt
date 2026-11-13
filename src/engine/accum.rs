//! 交易与权益曲线的在线累计器。

use super::scan::CloseOutcome;
use super::types::ExitReason;

/// 已平仓交易的累计量。非 detailed 路径只需要它, 不需要保留逐笔 [`Trade`](super::types::Trade)。
#[derive(Debug, Default)]
pub(super) struct TradeAccum {
    pub(super) num_trades: u32,
    /// 固定百分比/ATR 止损离场笔数。
    pub(super) stop_loss_exits: u32,
    /// 百分比/ATR 追踪或保本止损离场笔数。
    pub(super) trailing_stop_exits: u32,
    /// 策略止损线离场笔数。
    pub(super) strategy_stop_exits: u32,
    /// 逐笔 `pnl`: SQN / skew / kurtosis / pf-stability 的分半统计都要用它。
    ///
    /// **统计序列用 `f64`**: 这些是**统计量**(偏度/峰度/SQN), 本身是连续量,
    /// 不是"账上的钱"。账上的钱(累计 `total_fixed_pnl`)是 `Amount`。
    pub(super) pnls: Vec<f64>,
    /// 逐笔 `fixed_pnl`: fixed 腿的 win/loss 拆分与 expectancy(统计序列)。
    pub(super) fixed_pnls: Vec<f64>,
    /// 逐笔 `pnl_pct`(毛收益率, 与仓位大小无关): Kelly 仓位的估计样本。
    pub(super) pnl_pcts: Vec<f64>,
    /// 累计 fixed 腿盈亏(**金额**, 定点十进制)。
    pub(super) total_fixed_pnl: crate::money::Amount,
    /// 盈利笔累计盈亏(**金额**, 定点十进制)。
    pub(super) total_win: crate::money::Amount,
    /// 亏损笔累计亏损绝对值(**金额**, 定点十进制)。
    pub(super) total_loss: crate::money::Amount,
    /// 逐笔 `pnl_pct` 之和(仅用于 avg pnl_pct; 无需保留整个序列)。
    pub(super) pnl_pct_sum: f64,
    /// MFE / MAE / 持仓 bar 数之和。**报告列 `avg_mfe`/`avg_mae`/`avg_hold_bars`
    /// 需要它们**, 故非 detailed 路径也必须累计(只需三个标量, 不必留逐笔 Trade)。
    pub(super) sum_mfe: f64,
    pub(super) sum_mae: f64,
    pub(super) sum_hold: f64,
}

/// 单根 bar 的对数收益, 含**定义域保护**(两侧都要查)。
///
/// 空头持仓期间权益可能 ≤ 0(浮空)。此时比值 `curr/prev` 为负, `ln()` 返回 NaN;
/// NaN 经 Welford 污染 `ret_mean`/`ret_m2` 后会让 `ret_m2 > 0.0` 守卫失效,
/// 于是 sharpe/volatility 被**静默归零**(比 NaN 更危险: NaN 会暴露, 0 会被读成
/// "零波动低风险"), 同时 sortino 直接输出 NaN。故 `prev <= 0`、`curr <= 0`
/// 或任一侧非有限时记 0(退化而非污染)。
///
/// 注意: 只查 `prev` 不够 —— 首根即浮空(空头开仓当根权益转负)时 `prev` 仍为正,
/// `curr/prev < 0` 依旧产出 NaN。
#[inline]
fn log_ret(prev: f64, curr: f64) -> f64 {
    if !prev.is_finite() || !curr.is_finite() || prev <= 0.0 || curr <= 0.0 {
        0.0
    } else {
        (curr / prev).ln()
    }
}

/// 权益曲线 → 逐 bar 对数收益。引擎全部夏普口径的唯一来源; `pbo.rs::to_returns`
/// 也必须调用本函数, 否则 CSCV 的 IS/OOS 排序与引擎排名不同源。
///
/// 额外暴露 `Clone` 边界: 调用方需要在**不物化**收益数组的前提下做两遍统计
/// (先求均值、再求离差平方和), 逐位等价于「物化后单遍遍历」, 但省掉 O(T) 内存。
pub fn bar_log_returns(curve: &[f64]) -> impl ExactSizeIterator<Item = f64> + Clone + '_ {
    curve.windows(2).map(|w| log_ret(w[0], w[1]))
}

/// 权益曲线的**在线**统计累加器: O(1) 内存, 逐 bar [`CurveAccum::push`]。
///
/// 与「物化曲线后单遍遍历」**逐位一致**, 故 [`simulate`](super::simulate) 可省掉 O(T) 的曲线内存。
///
/// 动机: `simulate()` 只需要统计量, 不需要曲线本身。物化整条曲线
/// (`Vec<f64>`, 每 bar 8 B; 复利腿 + fixed 腿 → T×16 B) 后再单遍扫描,
/// 在 T = 1M 时每参数多占 16 MB。参数网格是**串行**外循环(见
/// `docs/invariants.md` 第 1 条, 策略层并行会 OOM), 故峰值内存直接决定 OOM 边界。
///
/// 正确性: 在线累计与「物化曲线后单遍遍历」**逐位一致**, 由
/// `test_curve_accum_is_bit_identical_to_materialized_reference` 对 5 类曲线
/// (单调上涨 / 锯齿 / 首根即持仓 / 穿零 / 含 NaN) 锁定。故这是纯内存优化,
/// **不改变任何指标数值**。
#[derive(Debug, Clone)]
pub struct CurveAccum {
    initial_cash: f64,
    /// 已喂入的点数(含初始点)。
    n: usize,
    ret_mean: f64,
    ret_m2: f64,
    ret_m2_downside: f64,
    n_downside: f64,
    peak: f64,
    max_dd: f64,
    last: f64,
}

/// 曲线统计结果。复利腿与 fixed 腿共用同一口径, 保证两套 Sharpe 可比。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveStats {
    /// 年化夏普。
    pub sharpe: f64,
    /// Sortino 比率(下行偏差口径, ddof=1)。
    pub sortino: f64,
    /// 最大回撤(小数)。
    pub max_dd: f64,
    /// 年化波动率, 百分数。
    pub volatility: f64,
    /// 总收益率(小数, 非百分数)。
    pub total_return: f64,
}

impl TradeAccum {
    /// 记一笔已平仓交易。
    ///
    /// `pnls` 必须保留: `compute_metrics` 的 SQN / skewness / kurtosis /
    /// pf-stability 分半统计都需要逐笔 pnl。其余字段都可以从它导出, 故不重复存。
    pub(super) fn push(&mut self, o: CloseOutcome) -> crate::error::Result<()> {
        self.num_trades += 1;
        match o.reason {
            ExitReason::StopLoss => self.stop_loss_exits += 1,
            ExitReason::TrailingStop => self.trailing_stop_exits += 1,
            ExitReason::StrategyStop => self.strategy_stop_exits += 1,
            ExitReason::Signal | ExitReason::EndOfData => {}
        }
        self.pnls.push(o.pnl.to_f64());
        self.fixed_pnls.push(o.fixed_pnl.to_f64());
        self.pnl_pcts.push(o.pnl_pct);
        self.total_fixed_pnl = self
            .total_fixed_pnl
            .plus(o.fixed_pnl)
            .map_err(|e| crate::error::AppError::Msg(format!("累计 fixed pnl 溢出: {e}")))?;
        if o.pnl > crate::money::Amount::ZERO {
            self.total_win = self
                .total_win
                .plus(o.pnl)
                .map_err(|e| crate::error::AppError::Msg(format!("累计盈利溢出: {e}")))?;
        } else if o.pnl < crate::money::Amount::ZERO {
            // 保本(pnl == 0)既不计盈利也不计亏损: 旧实现走 `else` 分支,
            // 把 0 记成一次"亏损"(金额为 0), 于是 avg_loss 的分母被稀释,
            // 胜率与连亏也被拉低/拉长。
            self.total_loss = self
                .total_loss
                .plus(o.pnl.abs())
                .map_err(|e| crate::error::AppError::Msg(format!("累计亏损溢出: {e}")))?;
        }
        self.pnl_pct_sum += o.pnl_pct;
        self.sum_mfe += o.max_runup;
        self.sum_mae += o.max_drawdown;
        self.sum_hold += o.bars_held as f64;
        Ok(())
    }
}

impl CurveAccum {
    /// 以初始资金建立累加器。
    #[must_use]
    pub fn new(initial_cash: f64) -> Self {
        Self {
            initial_cash,
            n: 0,
            ret_mean: 0.0,
            ret_m2: 0.0,
            ret_m2_downside: 0.0,
            n_downside: 0.0,
            peak: initial_cash,
            max_dd: 0.0,
            last: initial_cash,
        }
    }

    /// 喂入一根 bar 的权益值。
    ///
    /// 首点只登记基线, **不参与** peak/max_dd 统计 —— 与「`peak` 初值为
    /// `initial_cash` 且循环从 index 1 起」的物化口径一致。
    pub fn push(&mut self, eq: f64) {
        if self.n == 0 {
            self.n = 1;
            self.last = eq;
            return;
        }
        let ret = log_ret(self.last, eq);
        let k = self.n as f64;
        let delta = ret - self.ret_mean;
        self.ret_mean += delta / k;
        self.ret_m2 += delta * (ret - self.ret_mean);
        if ret < 0.0 {
            self.ret_m2_downside += ret * ret;
            self.n_downside += 1.0;
        }
        if eq > self.peak {
            self.peak = eq;
        }
        let dd = (self.peak - eq) / self.peak;
        if dd > self.max_dd {
            self.max_dd = dd;
        }
        self.last = eq;
        self.n += 1;
    }

    /// 已喂入的点数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否尚未喂入任何点。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 末点权益。
    #[must_use]
    pub fn last(&self) -> f64 {
        self.last
    }

    /// 逐 bar 对数收益口径的全部曲线统计。
    #[must_use]
    pub fn stats(&self, bars_per_year: f64) -> CurveStats {
        let num_bars = (self.n.saturating_sub(1)) as f64;
        let total_return = if self.initial_cash > 0.0 {
            (self.last - self.initial_cash) / self.initial_cash
        } else {
            0.0
        };
        if self.n < 2 {
            return CurveStats {
                sharpe: 0.0,
                sortino: 0.0,
                max_dd: 0.0,
                volatility: 0.0,
                total_return,
            };
        }
        // `is_finite` 守卫不可省: 见 [`log_ret`] 的说明 —— NaN 会让 `> 0.0` 为假,
        // 但 `-inf` / 极大值会让它为真, 从而把非有限值推进方差。
        let (sharpe, volatility) = if self.ret_m2.is_finite() && self.ret_m2 > 0.0 && num_bars > 1.0
        {
            let variance = self.ret_m2 / (num_bars - 1.0);
            (
                self.ret_mean / variance.sqrt() * bars_per_year.sqrt(),
                variance.sqrt() * bars_per_year.sqrt() * 100.0,
            )
        } else {
            (0.0, 0.0)
        };
        // 下行偏差按**下行 bar 数**归一(ddof=1), 与 sharpe 同一自由度约定。
        let sortino = if self.ret_m2_downside.is_finite()
            && self.ret_m2_downside > 0.0
            && self.n_downside > 1.0
        {
            let downside_var = self.ret_m2_downside / (self.n_downside - 1.0);
            self.ret_mean / downside_var.sqrt() * bars_per_year.sqrt()
        } else {
            0.0
        };
        CurveStats {
            sharpe,
            sortino,
            max_dd: self.max_dd,
            volatility,
            total_return,
        }
    }
}

/// 仅测试用: 「物化曲线后单遍遍历」的参照实现。
///
/// [`CurveAccum`] 的正确性基准。它刻意用与累加器**不同**的写法
/// (先物化 `Vec`、再用索引遍历), 使"两者同时写错同一处"不可能发生。
#[cfg(test)]
pub(super) fn reference_curve_stats(
    curve: &[f64],
    initial_cash: f64,
    bars_per_year: f64,
) -> CurveStats {
    let n = curve.len();
    if n < 2 {
        return CurveStats {
            sharpe: 0.0,
            sortino: 0.0,
            max_dd: 0.0,
            volatility: 0.0,
            total_return: 0.0,
        };
    }
    let mut ret_mean = 0.0;
    let mut ret_m2 = 0.0;
    let mut ret_m2_downside = 0.0;
    let mut n_downside = 0.0;
    let mut peak = initial_cash;
    let mut max_dd = 0.0;
    for i in 1..n {
        let ret = bar_log_returns(&curve[i - 1..i + 1]).next().unwrap_or(0.0);
        let delta = ret - ret_mean;
        ret_mean += delta / i as f64;
        ret_m2 += delta * (ret - ret_mean);
        if ret < 0.0 {
            ret_m2_downside += ret * ret;
            n_downside += 1.0;
        }
        if curve[i] > peak {
            peak = curve[i];
        }
        let dd = (peak - curve[i]) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    let num_bars = (n - 1) as f64;
    let (sharpe, volatility) = if ret_m2.is_finite() && ret_m2 > 0.0 && num_bars > 1.0 {
        let variance = ret_m2 / (num_bars - 1.0);
        (
            ret_mean / variance.sqrt() * bars_per_year.sqrt(),
            variance.sqrt() * bars_per_year.sqrt() * 100.0,
        )
    } else {
        (0.0, 0.0)
    };
    let sortino = if ret_m2_downside.is_finite() && ret_m2_downside > 0.0 && n_downside > 1.0 {
        let downside_var = ret_m2_downside / (n_downside - 1.0);
        ret_mean / downside_var.sqrt() * bars_per_year.sqrt()
    } else {
        0.0
    };
    let final_eq = *curve.last().unwrap_or(&initial_cash);
    CurveStats {
        sharpe,
        sortino,
        max_dd,
        volatility,
        total_return: (final_eq - initial_cash) / initial_cash,
    }
}
