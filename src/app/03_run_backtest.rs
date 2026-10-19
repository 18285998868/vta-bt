use std::num::NonZeroUsize;

use rayon::prelude::*;

use crate::strategies::Signal;

/// 单次求值的全部回测指标。
///
/// **单位约定**: 带 `_pct` 的字段是百分数(15.0 = 15%), `total_return` /
/// `win_rate` 是小数(0.15 = 15%)。落盘时按 [`output::COLUMNS`](crate::output)
/// 声明的单位再折算一次, 该表由测试锁定与 schema 一致。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SummaryMetrics {
    /// 总收益率(百分数, 15.0 = 15%)。
    pub total_return_pct: f64,
    /// 总收益率(小数)。`= total_return_pct / 100`。
    pub total_return: f64,
    /// 年化夏普(逐 bar 对数收益, ddof=1 × √bars_per_year)。
    pub sharpe_ratio: f64,
    /// 下行偏差口径, 分母为**下行 bar 数**的样本方差(ddof=1), 与 `sharpe_ratio`
    /// 同一自由度约定, 两者可并列比较。
    pub sortino_ratio: f64,
    /// 最大回撤(百分数)。
    pub max_drawdown_pct: f64,
    /// 盈利笔数占比(百分数)。
    pub win_rate: f64,
    /// 总盈利 / 总亏损。无亏损时取总盈利。
    pub profit_factor: f64,
    /// CAGR / 最大回撤。无回撤时为 0。
    pub calmar_ratio: f64,
    /// 已平仓交易笔数(往返各计一次)。
    pub num_trades: u32,
    /// 盈利笔的平均 pnl(货币)。
    pub avg_win: f64,
    /// 亏损笔的平均亏损**绝对值**(货币, 正数)。
    pub avg_loss: f64,
    /// 逐笔 `pnl_pct` 的均值(小数)。
    pub expectancy: f64,
    /// 最长连��盈利笔数。
    pub consecutive_wins: u32,
    /// 最长连亏笔数。
    pub consecutive_losses: u32,
    /// 末根权益(货币)。空头浮空时可为负。
    pub final_equity: f64,
    /// 年化波动率(百分数)。
    pub volatility: f64,
    /// 总收益 / 最大回撤。
    pub recovery_factor: f64,
    /// 平均持仓 bar 数。
    pub avg_hold_bars: f64,
    /// 平均最大不利偏移(价格差)。
    pub avg_mae: f64,
    /// 平均最大有利偏移(价格差)。
    pub avg_mfe: f64,
    /// `avg_mfe / avg_mae`。avg_mae=0 时为 0。
    pub edge_ratio: f64,
    /// 平均盈利 / 平均亏损。
    pub wl_ratio: f64,
    /// 逐笔 pnl 的样本偏度(小样本校正)。
    pub skewness: f64,
    /// 逐笔 pnl 的超额峰度(Fisher,小样本校正)。
    pub kurtosis: f64,
    /// SQN = √n · mean/stdev (Van Thorp 1998)。**仅在 `num_trades` >= 30 时有意义**
    /// (Van Thorp 建议门槛); n 越大 SQN 越高是统计显著性的体现, 非过拟合信号。
    /// 判读时必须与 `num_trades` 配对。
    pub sqn: f64,
    /// 前后半段 profit factor 的比值(≤1)。样本 <20 时为 0。
    pub pf_stability: f64,
    /// 连盈连亏段数的 runs 检验 z 值。
    pub z_score: f64,
    /// runs 检验双尾 p 值。
    pub p_value: f64,
    /// fixed 腿(固定名义、不复利)总盈亏。
    pub total_fixed_pnl: f64,
    /// 年化成交笔数(往返次数)。信号抖动的主要预警指标:
    /// 平均持仓不足 5 根 bar 时, 该值会高到成本必然吞掉全部毛利。
    pub turnover_per_year: f64,
    /// 执行口径(回放用): 信号延迟 bar 数。
    /// 执行口径: 信号到成交的 bar 间隔(恒 ≥1)。
    pub delay_bars: u32,
    /// 执行口径(回放用): 单边佣金 bp。
    /// 执行口径: 单边佣金(bp)。
    pub fee_bps: f64,
    /// 执行口径(回放用): 单边逆向滑点 bp。
    /// 执行口径: 单边逆向滑点(bp)。
    pub slippage_bps: f64,
    /// fixed 腿总收益(固定名义, 不复利)。研究报告引用此值, 而非复利 total_return。
    /// fixed 腿总收益率(小数)。
    pub fixed_total_return: f64,
    /// fixed 腿年化 Sharpe(与 sharpe_ratio 同公式, 曲线为 fixed 盯市)。
    /// fixed 腿年化夏普。
    pub fixed_sharpe_ratio: f64,
    /// fixed 腿最大回撤 %。
    /// fixed 腿最大回撤(百分数)。
    pub fixed_max_drawdown_pct: f64,
    /// fixed 腿单笔期望(bp, 以 initial 计)。C6 成本分析的直接输入。
    /// fixed 腿单笔期望(bp, 以初始资金计)。
    pub fixed_expectancy_bps: f64,
    /// fixed 腿 profit factor。
    pub fixed_profit_factor: f64,
}

/// 成交执行假设。`delay_bars = 0` 为旧口径(信号当根收盘成交, 前视乐观);
/// `delay_bars = 1` 为推荐口径(信号次根 fill bar 成交, `fill_close=false` 取开盘,
/// `fill_close=true` 取收盘)。费率按单边名义扣除。
///
/// **`delay_bars` 恒 ≥ 1**: 信号次根 fill bar 成交。`delay_bars = 0`(信号当根收盘成交)
/// 属前视乐观口径, 已在**引擎层彻底移除** —— 字段是 [`NonZeroUsize`], 类型上无法
/// 构造 0。费率按单边名义扣除。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecConfig {
    /// 信号到成交的 bar 间隔, 恒 ≥ 1(类型保证)。1 = 次根 fill bar。
    pub delay_bars: NonZeroUsize,
    /// 单边佣金率(= bps / 10000)。
    pub fee_rate: f64,
    /// 单边逆向滑点率(= bps / 10000)。
    pub slip_rate: f64,
    /// fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
}

impl Default for ExecConfig {
    /// 推荐口径: 次根 fill bar 成交(开盘价)、无成本。
    fn default() -> Self {
        Self {
            delay_bars: NonZeroUsize::MIN,
            fee_rate: 0.0,
            slip_rate: 0.0,
            fill_close: false,
        }
    }
}

impl ExecConfig {
    /// 构造执行配置。`delay_bars = 0` 直接拒绝(前视乐观口径已删除)。
    ///
    /// # Errors
    /// `delay_bars == 0` 时返回 [`AppError::Msg`](crate::error::AppError::Msg)。
    pub fn try_from_bps_fill(
        delay_bars: usize,
        fee_bps: f64,
        slippage_bps: f64,
        fill_close: bool,
    ) -> crate::error::Result<Self> {
        let delay_bars = NonZeroUsize::new(delay_bars).ok_or_else(|| {
            crate::error::AppError::Msg(
                "delay_bars 必须 >= 1: 信号当根成交为前视乐观口径, 已移除".to_string(),
            )
        })?;
        Ok(Self {
            delay_bars,
            fee_rate: fee_bps / 10_000.0,
            slip_rate: slippage_bps / 10_000.0,
            fill_close,
        })
    }

    /// [`Self::try_from_bps_fill`] 的 infallible 版本(固定 `fill_close = false`)。
    ///
    /// # Panics
    /// `delay_bars == 0` 时 panic(前视乐观口径已移除)。
    ///
    /// 这个 `expect` 是**故意的"bug 探测器"**(`err-expect-bugs-only`):
    /// `delay_bars == 0` 不是用户输入错误 —— 配置路径在**反序列化期**就把它
    /// 拒了(`BacktestConfig::try_from`), 能走到这里的 `0` 一定是代码写错了,
    /// 应当立刻炸掉而不是返回一个"前视乐观"的执行口径。
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn from_bps_fill(
        delay_bars: usize,
        fee_bps: f64,
        slippage_bps: f64,
        fill_close: bool,
    ) -> Self {
        Self::try_from_bps_fill(delay_bars, fee_bps, slippage_bps, fill_close)
            .expect("delay_bars >= 1: 前视乐观口径(delay=0)已在引擎层移除")
    }

    /// [`Self::from_bps_fill`] 的 `fill_close = false` 简写。
    ///
    /// # Panics
    /// 同 [`Self::from_bps_fill`]。
    #[must_use]
    pub fn from_bps(delay_bars: usize, fee_bps: f64, slippage_bps: f64) -> Self {
        Self::from_bps_fill(delay_bars, fee_bps, slippage_bps, false)
    }

    /// 信号到成交的 bar 间隔(恒 ≥1)。
    #[must_use]
    pub fn delay(&self) -> usize {
        self.delay_bars.get()
    }

    /// 信号到成交的 bar 间隔, 收窄为报告列用的 `u32`。
    ///
    /// **不静默截断**: `usize → u32` 的 `as` 转换在 `delay > u32::MAX` 时会截成 0,
    /// 而 0 恰是本项目明令禁止的"前视乐观"标记值 —— 报告口径会与执行口径**静默
    /// 不一致**。这里改为饱和到 `u32::MAX`, 宁可报一个明显偏大的数, 也不报 0。
    #[must_use]
    pub fn delay_u32(&self) -> u32 {
        u32::try_from(self.delay_bars.get()).unwrap_or(u32::MAX)
    }
}

/// 持仓/订单方向。
///
/// 原实现用 `i8` 的 `+1/-1/0`, 但 `i8` 能表示 `2`、`-2`、`127` 等**引擎无法处理**
/// 的值: `fill_price(side==1)` 之外的任何值都走"卖出"分支, `cur_pos != 1 && != -1`
/// 的分支则被当作平仓 —— 两处都会静默算错而不报错。enum 让非法方向**不可表示**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// 无持仓。
    #[default]
    Flat,
    /// 多头。
    Long,
    /// 空头。
    Short,
}

impl Side {
    /// 符号: Long = +1, Short = -1, Flat = 0。
    #[must_use]
    pub fn sign(self) -> i8 {
        match self {
            Self::Flat => 0,
            Self::Long => 1,
            Self::Short => -1,
        }
    }

    /// 由 `+1` / `-1` / 其他 还原方向。
    #[must_use]
    pub fn from_sign(s: i8) -> Self {
        match s {
            1 => Self::Long,
            -1 => Self::Short,
            _ => Self::Flat,
        }
    }

    /// 反向(用于平仓: 取当前持仓的相反方向下单)。
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::Flat => Self::Flat,
            Self::Long => Self::Short,
            Self::Short => Self::Long,
        }
    }

    /// 是否处于持仓(`Long` 或 `Short`)。
    #[must_use]
    pub fn is_open(self) -> bool {
        self != Self::Flat
    }

    /// 报告/落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
            Self::Flat => "FLAT",
        }
    }

    /// 由信号推出的持仓方向。
    #[must_use]
    pub fn from_signal(s: Signal) -> Self {
        match s {
            Signal::Buy => Self::Long,
            Signal::Sell => Self::Short,
            Signal::Close | Signal::Hold => Self::Flat,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
/// 一笔已平仓交易。仅 `--detail` 路径产出(非详细路径只累计标量统计)。
pub struct Trade {
    /// 开仓 bar 的时间戳(ms)。
    pub entry_time: i64,
    /// 平仓 bar 的时间戳(ms)。
    pub exit_time: i64,
    /// 持仓方向: `Long` / `Short`。恒不为 `Flat`。
    pub side: Side,
    /// 开仓成交价(含逆向滑点)。
    pub entry_price: f64,
    /// 平仓成交价(含逆向滑点)。
    pub exit_price: f64,
    /// 复利腿持仓数量。
    pub qty: f64,
    /// 复利腿盈亏(货币, 已扣双边佣金)。
    pub pnl: f64,
    /// 相对开仓价的收益率(小数)。
    pub pnl_pct: f64,
    /// fixed 腿盈亏(固定名义, 已扣双边佣金)。
    pub fixed_pnl: f64,
    /// 持仓 bar 数。
    pub bars_held: i64,
    /// 持仓期内最大有利偏移(价格差)。
    pub max_runup: f64,
    /// 持仓期内最大不利偏移(价格差)。
    pub max_drawdown: f64,
    /// 平仓原因。取值集合是**封闭**的(见 [`ExitReason`]), 故用 `&'static str`
    /// 而非 `String`: 原实现每个平仓都 `format!`-出一个堆 `String`, 而参数网格
    /// 动辄几十万笔, 这是一次纯浪费的分配 + 拷贝。
    pub exit_reason: &'static str,
}

/// 平仓原因的封闭集合。
///
/// 枚举化是为了让 `exit_reason` 的取值**在编译期封闭** —— 落盘层的字典序统计
/// 不再需要防御未知字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// 持仓被反向信号平掉。
    Signal,
    /// 数据末尾强制平仓。
    EndOfData,
}

impl ExitReason {
    /// 落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::EndOfData => "end_of_data",
        }
    }
}

#[derive(Debug, Clone)]
/// `--detail` 路径的一次求值结果。
pub struct SimResult {
    /// 全部指标(与 [`simulate`] 逐位一致)。
    pub summary: SummaryMetrics,
    /// 逐笔已平仓交易。
    pub trades: Vec<Trade>,
    /// 复利腿逐 bar `(时间戳, 权益)`。
    pub equity_curve: Vec<(f64, f64)>,
    /// fixed 腿逐 bar 权益(与 bar 对齐, `--pbo` 的 T×N 输入)。
    /// fixed 腿逐 bar 权益(与 bar 对齐)。
    pub fixed_curve: Vec<f64>,
}

/// 信号 → **目标持仓方向** 的单步状态机。
///
/// 为什么不是一个数组: 原实现先物化一份 O(T) 的 `Vec<Side>`, 而 `scan_trades`
/// 只在**当前 bar** 用到 `positions[i]`。物化版本的代价是每次参数求值多分配
/// 一条与 bar 数等长的向量(T=1M 时 1 MB/档), 参数网格动辄 10 万档 ⇒ 百 GB 级
/// 的额外分配流量, 与本仓库"内存是第一约束"的设计前提直接冲突。
///
/// 状态机是**前缀累积**的, 故逐 bar 增量维护与先物化再索引**逐位等价**。
#[inline]
fn next_target(prev: Side, sig: Signal) -> Side {
    match sig {
        Signal::Buy => Side::Long,
        Signal::Sell => Side::Short,
        Signal::Close => Side::Flat,
        Signal::Hold => prev,
    }
}

#[derive(Debug, Clone, Copy)]
struct CloseArgs {
    cur_pos: Side,
    pos_qty: f64,
    pos_entry: f64,
    pos_entry_time: i64,
    pos_entry_bar: usize,
    exit_price: f64,
    exit_time: i64,
    exit_bar: usize,
    initial_cash: f64,
    trade_mfe: f64,
    trade_mae: f64,
    fee_rate: f64,
    /// 平仓原因(信号反手 / 数据末尾强制)。
    reason: ExitReason,
}

/// 方向 side 的成交价: 买入抬价、卖出压价(逆向滑点)。
fn fill_price(side: Side, base: f64, slip_rate: f64) -> f64 {
    if side == Side::Long {
        base * (1.0 + slip_rate)
    } else {
        base * (1.0 - slip_rate)
    }
}

/// 被动基准: 首根 close 买入、末根 close 卖出, 无成本, 固定名义。
/// 返回 (pnl, return)。数据不足 2 根或首价非正有限值时返回 (0, 0)。
/// 与策略 fixed 腿同为固定名义口径, 可直接对比。
#[must_use]
pub fn buy_hold_pnl(close: &[f64], nominal: f64) -> (f64, f64) {
    if close.len() < 2 || !nominal.is_finite() || nominal <= 0.0 {
        return (0.0, 0.0);
    }
    let (first, last) = (close[0], close[close.len() - 1]);
    if !first.is_finite() || !last.is_finite() || first <= 0.0 {
        return (0.0, 0.0);
    }
    let ret = last / first - 1.0;
    (ret * nominal, ret)
}

/// 成交价是否**可用**: 有限且为正。
///
/// 成交价是整条成交链的除数: `qty = cash / price`、`pnl_pct = (exit - entry) / entry`。
/// 价格为 0 ⇒ `qty = inf` ⇒ 权益变 NaN ⇒ **全部指标被污染**; 且 NaN 比 0 更危险 ——
/// 0 会被读成"零波动低风险", NaN 才会暴露。价格非有限(NaN/Inf)同理。
///
/// 数据层的非法价格在**加载边界**被拒(见 `app::validate_columns`); 此处是引擎
/// 侧的兜底, 保证引擎被单独调用(库用户)时也不会产出 NaN 指标。
#[inline]
fn valid_fill_price(p: f64) -> bool {
    p.is_finite() && p > 0.0
}

/// 信号 bar `i`、订单方向 `side` 的**开仓**成交位置与价格。
///
/// `delay_bars` 恒 ≥ 1(`NonZeroUsize` 保证), 故成交只可能落在 fill bar
/// `i + delay_bars`, 无"当根成交"分支:
/// - fill bar 的开盘(`fill_close=false`, 默认)或收盘(`fill_close=true`);
/// - 开仓 fill bar 超出数据 → `None`(放弃, 无未来可成交);
/// - 成交价非有限或非正 → `None`(数据异常, 不成交)。
fn resolve_open_fill(
    i: usize,
    side: Side,
    close: &[f64],
    open: &[f64],
    n: usize,
    exec: &ExecConfig,
) -> Option<(usize, f64)> {
    let fb = i + exec.delay();
    if fb >= n {
        return None;
    }
    let base = if exec.fill_close {
        close[fb]
    } else {
        open.get(fb).copied().unwrap_or(close[fb])
    };
    let price = fill_price(side, base, exec.slip_rate);
    if !valid_fill_price(price) {
        return None;
    }
    Some((fb, price))
}

/// 信号 bar `i`、订单方向 `side` 的**平仓**成交位置与价格。
///
/// 与 [`resolve_open_fill`] 的差别: 平仓永远发生在数据末端之内, 故 fill bar
/// 超出范围时**钳到末根**(与旧口径的末根离场一致), 不返回 `None`。
///
/// 成交价非有限或非正时同样返回 `None` —— 调用方据此**放弃本次平仓**(持仓延续
/// 到下一根), 而不是拿一个会污染全部指标的 NaN/0 价格去结算。
fn resolve_close_fill(
    i: usize,
    side: Side,
    close: &[f64],
    open: &[f64],
    n: usize,
    exec: &ExecConfig,
) -> Option<(usize, f64)> {
    let fb = i + exec.delay();
    let (base_bar, base) = if fb >= n {
        let last = n - 1;
        (last, close[last])
    } else {
        let base = if exec.fill_close {
            close[fb]
        } else {
            open.get(fb).copied().unwrap_or(close[fb])
        };
        (fb, base)
    };
    let price = fill_price(side, base, exec.slip_rate);
    if !valid_fill_price(price) {
        return None;
    }
    Some((base_bar, price))
}

/// 一笔平仓的**标量**结果。
///
/// 与 [`Trade`] 分离的原因: `simulate()`(非 detailed 路径)只需要标量来算统计量,
/// 根本不需要 `Trade`。原先两者被绑在一起, 导致每个参数求值都要为每笔交易
/// 构造一个 `Trade` —— 其中 `exit_reason: String` 是**堆分配**。参数网格是串行
/// 外循环(策略层并行会 OOM, 见 README), 这笔开销直接乘以总交易笔数。
#[derive(Debug, Clone, Copy)]
struct CloseOutcome {
    cash_delta: f64,
    pnl: f64,
    pnl_pct: f64,
    fixed_pnl: f64,
    bars_held: i64,
    max_runup: f64,
    max_drawdown: f64,
}

/// 已平仓交易的累计量。非 detailed 路径只需要它, 不需要保留逐笔 [`Trade`]。
#[derive(Debug, Default)]
struct TradeAccum {
    num_trades: u32,
    /// 逐笔 `pnl`: SQN / skew / kurtosis / pf-stability 的分半统计都要用它。
    pnls: Vec<f64>,
    /// 逐笔 `fixed_pnl`: fixed 腿的 win/loss 拆分与 expectancy。
    fixed_pnls: Vec<f64>,
    total_fixed_pnl: f64,
    /// 逐笔 `pnl_pct` 之和(仅用于 avg pnl_pct; 无需保留整个序列)。
    pnl_pct_sum: f64,
    /// MFE / MAE / 持仓 bar 数之和。**报告列 `avg_mfe`/`avg_mae`/`avg_hold_bars`
    /// 需要它们**, 故非 detailed 路径也必须累计(只需三个标量, 不必留逐笔 Trade)。
    sum_mfe: f64,
    sum_mae: f64,
    sum_hold: f64,
}

fn close_outcome(args: CloseArgs) -> CloseOutcome {
    let CloseArgs {
        cur_pos,
        pos_qty,
        pos_entry,
        pos_entry_bar,
        exit_price,
        exit_bar,
        initial_cash,
        trade_mfe,
        trade_mae,
        fee_rate,
        ..
    } = args;

    // 平仓单方向与持仓反向: 多头平仓为卖, 空头平仓为买; 佣金按单边名义扣除。
    let exit_fee = exit_price * pos_qty * fee_rate;
    let (cash_delta, pnl) = if cur_pos == Side::Long {
        (
            pos_qty * exit_price - exit_fee,
            (exit_price - pos_entry) * pos_qty - exit_fee,
        )
    } else {
        (
            -(exit_price * pos_qty) - exit_fee,
            (pos_entry - exit_price) * pos_qty - exit_fee,
        )
    };
    let fixed_qty = initial_cash / pos_entry;
    let entry_fee_fixed = pos_entry * fixed_qty * fee_rate;
    let exit_fee_fixed = exit_price * fixed_qty * fee_rate;
    let fixed_pnl = if cur_pos == Side::Long {
        (exit_price - pos_entry) * fixed_qty - entry_fee_fixed - exit_fee_fixed
    } else {
        (pos_entry - exit_price) * fixed_qty - entry_fee_fixed - exit_fee_fixed
    };
    let pnl_pct = if cur_pos == Side::Long {
        (exit_price - pos_entry) / pos_entry
    } else {
        (pos_entry - exit_price) / pos_entry
    };
    CloseOutcome {
        cash_delta,
        pnl,
        pnl_pct,
        fixed_pnl,
        bars_held: (exit_bar as i64) - (pos_entry_bar as i64),
        max_runup: trade_mfe,
        max_drawdown: trade_mae,
    }
}

/// 由标量结果物化完整 [`Trade`]。**仅 detailed 路径调用**。
fn materialize_trade(args: &CloseArgs, out: CloseOutcome) -> Trade {
    Trade {
        entry_time: args.pos_entry_time,
        exit_time: args.exit_time,
        side: args.cur_pos,
        entry_price: args.pos_entry,
        exit_price: args.exit_price,
        qty: args.pos_qty,
        pnl: out.pnl,
        pnl_pct: out.pnl_pct,
        fixed_pnl: out.fixed_pnl,
        bars_held: out.bars_held,
        max_runup: out.max_runup,
        max_drawdown: out.max_drawdown,
        exit_reason: args.reason.as_str(),
    }
}

/// 指标计算的**唯一**数据源。
///
/// 两条路径(详细 / 非详细)**都**填充它, 且填充方式逐行相同 —— 这是
/// "报告数字不随 `--detail` 开关变化"的结构性保证: [`compute_metrics`] 的签名
/// 收 `&MetricsSource`, 因此它在**类型上就看不到** [`DetailArtifacts`]。
///
/// 拆分自原先的单一 `ScanOut`: 那里 6 个字段并列, "指标只读其中 3 个"仅仅是
/// 约定(谁新增一个字段并顺手读它, 就会让两条路径漂移), 而非编译期约束。
#[derive(Debug)]
struct MetricsSource {
    acc: TradeAccum,
    eq: CurveAccum,
    fx: CurveAccum,
}

/// 仅详细路径需要落盘的产物。
///
/// 为空表示 `collect_curves == false`([`simulate`]); [`simulate_detailed`] 传
/// true 时填充。这些字段**不参与**任何指标计算 —— 指标只读 [`MetricsSource`]。
#[derive(Debug, Default)]
struct DetailArtifacts {
    trades: Vec<Trade>,
    equity: Vec<f64>,
    fixed: Vec<f64>,
}

/// `scan_trades` 的产出: 指标源 + (可选的)落盘产物。
#[derive(Debug)]
struct ScanOut {
    /// 两条路径都有, 且填充方式完全一致。
    metrics: MetricsSource,
    /// 仅 `collect_curves == true` 时有内容; 否则三个字段全空。
    detail: DetailArtifacts,
    /// 是否物化了落盘产物。**不**用 `Option` 表达, 免得调用方为了取产物
    /// 而不得不写一个 `expect()`(那正是 `err-no-unwrap-prod` 反对的东西)。
    collect_curves: bool,
}

/// `collect_curves`: 是否物化两条权益曲线。false = O(1) 内存(见 [`CurveAccum`])。
fn scan_trades(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    time: &[i64],
    initial_cash: f64,
    exec: &ExecConfig,
    collect_curves: bool,
) -> ScanOut {
    let n = close.len().min(signals.len()).min(open.len());
    let ts = |i: usize| if i < time.len() { time[i] } else { i as i64 };
    let mut cash = initial_cash;
    let mut pos_qty = 0.0;
    let mut pos_entry = 0.0;
    let mut pos_entry_time: i64 = 0;
    let mut pos_entry_bar: usize = 0;
    let mut cur_pos = Side::Flat;
    // 目标持仓方向: 由信号序列前缀累积而来(见 [`next_target`])。
    // 增量维护, 不物化 O(T) 的 `Vec<Side>`。
    let mut target = Side::Flat;
    let mut trades: Vec<Trade> = if collect_curves {
        Vec::with_capacity(1024)
    } else {
        Vec::new()
    };
    let mut acc = TradeAccum {
        pnls: Vec::with_capacity(1024),
        fixed_pnls: Vec::with_capacity(1024),
        ..TradeAccum::default()
    };
    let mut eq_acc = CurveAccum::new(initial_cash);
    let mut fx_acc = CurveAccum::new(initial_cash);
    // 曲线容器: 不收集时保持为空(Vec::new() 不分配), 收集时一次预分配到位。
    // 刻意**不用** `Option<Vec<f64>>`: 那会让调用方为了取产物而写 `expect()`。
    let mut equity_curve: Vec<f64> = Vec::new();
    // fixed 腿: 每笔按 initial/entry 固定张数, 逐 bar 盯市, 跨笔不复利。
    let mut fixed_curve: Vec<f64> = Vec::new();
    if collect_curves {
        equity_curve.reserve(n);
        fixed_curve.reserve(n);
    }
    // 末根权益暂存: 强制平仓会把它从"盯市值"改写为"已实现 cash", 而在线累加器
    // 无法撤回已消费的点, 故末根**推迟**到平仓之后再入累加器/曲线。
    let mut tail_eq = initial_cash;
    let mut tail_feq = initial_cash;
    let mut fixed_realized = 0.0;
    let mut fixed_qty = 0.0;
    let mut fixed_entry = 0.0;

    let mut trade_mfe: f64 = 0.0;
    let mut trade_mae: f64 = 0.0;

    for i in 0..n {
        // 目标持仓方向(信号前缀累积): 与"先物化 positions 再索引"逐位等价。
        target = next_target(target, signals[i]);

        // 浮动盈亏只统计已成交持仓(fill bar 之后), fill 前的漂移不计入 MFE/MAE。
        if cur_pos.is_open() && i >= pos_entry_bar {
            let favor = if cur_pos == Side::Long {
                close[i] - pos_entry
            } else {
                pos_entry - close[i]
            };
            if favor > trade_mfe {
                trade_mfe = favor;
            }
            let adverse = -favor;
            if adverse > trade_mae {
                trade_mae = adverse;
            }
        }

        if cur_pos.is_open() && cur_pos != target {
            let exit_side = cur_pos.opposite();
            // 平仓 fill 落在数据末端之内(尾部钳到末根); 成交价非法(非有限/非正)时
            // 返回 None —— 此时**放弃本次平仓**(持仓延续到下一根), 而不是用会
            // 污染全部指标的 NaN/0 价格结算。
            if let Some((fb, exit_price)) = resolve_close_fill(i, exit_side, close, open, n, exec) {
                let cargs = CloseArgs {
                    cur_pos,
                    pos_qty,
                    pos_entry,
                    pos_entry_time,
                    pos_entry_bar,
                    exit_price,
                    exit_time: ts(fb),
                    exit_bar: fb,
                    initial_cash,
                    trade_mfe,
                    trade_mae,
                    fee_rate: exec.fee_rate,
                    reason: ExitReason::Signal,
                };
                let out = close_outcome(cargs);
                cash += out.cash_delta;
                acc.push(out);
                if collect_curves {
                    trades.push(materialize_trade(&cargs, out));
                }
                fixed_realized += out.fixed_pnl;
                fixed_qty = 0.0;
                cur_pos = Side::Flat;
                trade_mfe = 0.0;
                trade_mae = 0.0;
            }
        }

        if !cur_pos.is_open() && target.is_open() {
            let side = target;
            if let Some((fb, entry_price)) = resolve_open_fill(i, side, close, open, n, exec) {
                let qty = cash / entry_price;
                let fee = entry_price * qty * exec.fee_rate;
                pos_entry = entry_price;
                pos_entry_time = ts(fb);
                pos_entry_bar = fb;
                pos_qty = qty;
                fixed_qty = initial_cash / entry_price;
                fixed_entry = entry_price;
                if side == Side::Long {
                    cash -= qty * entry_price + fee;
                } else {
                    cash += qty * entry_price - fee;
                }
                cur_pos = side;
                trade_mfe = 0.0;
                trade_mae = 0.0;
            }
        }

        let eq = match cur_pos {
            Side::Long => cash + pos_qty * close[i],
            Side::Short => cash - pos_qty * close[i],
            Side::Flat => cash,
        };
        let feq = match cur_pos {
            Side::Long => initial_cash + fixed_realized + fixed_qty * (close[i] - fixed_entry),
            Side::Short => initial_cash + fixed_realized + fixed_qty * (fixed_entry - close[i]),
            Side::Flat => initial_cash + fixed_realized,
        };
        if i + 1 < n {
            eq_acc.push(eq);
            fx_acc.push(feq);
            if collect_curves {
                equity_curve.push(eq);
                fixed_curve.push(feq);
            }
        } else {
            tail_eq = eq;
            tail_feq = feq;
        }
    }

    if cur_pos.is_open() {
        // 末根强制平仓。成交价非法(非有限/非正)时不强行结算 —— 宁可留一笔未平仓
        // 持仓(末根权益按盯市值计), 也不用 NaN 污染全部指标。
        let last_price = fill_price(cur_pos.opposite(), close[n - 1], exec.slip_rate);
        if valid_fill_price(last_price) {
            let cargs = CloseArgs {
                cur_pos,
                pos_qty,
                pos_entry,
                pos_entry_time,
                pos_entry_bar,
                exit_price: last_price,
                exit_time: ts(n - 1),
                exit_bar: n - 1,
                initial_cash,
                trade_mfe,
                trade_mae,
                fee_rate: exec.fee_rate,
                reason: ExitReason::EndOfData,
            };
            let out = close_outcome(cargs);
            cash += out.cash_delta;
            acc.push(out);
            if collect_curves {
                trades.push(materialize_trade(&cargs, out));
            }
            fixed_realized += out.fixed_pnl;
            // 强制平仓: 末根权益由盯市值改写为已实现 cash(扣掉平仓费与滑点)。
            tail_eq = cash;
            tail_feq = initial_cash + fixed_realized;
        }
    }

    if n > 0 {
        eq_acc.push(tail_eq);
        fx_acc.push(tail_feq);
        if collect_curves {
            equity_curve.push(tail_eq);
            fixed_curve.push(tail_feq);
        }
    }

    ScanOut {
        metrics: MetricsSource {
            acc,
            eq: eq_acc,
            fx: fx_acc,
        },
        detail: DetailArtifacts {
            trades,
            equity: equity_curve,
            fixed: fixed_curve,
        },
        collect_curves,
    }
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
/// 动机: `simulate()` 只需要统计量, 不需要曲线本身。物化整条曲线
/// (`Vec<f64>`, 每 bar 8 B; 复利腿 + fixed 腿 → T×16 B) 后再单遍扫描,
/// 在 T = 1M 时每参数多占 16 MB。参数网格是**串行**外循环(见 README
/// 「关键设计约束」第 1 条, 策略层并行会 OOM), 故峰值内存直接决定 OOM 边界。
///
/// 正确性: 在线累计与「物化曲线后单遍遍历」**逐位一致**, 由
/// `test_curve_accum_is_bit_identical_to_materialized_reference` 对 5 类曲线
/// (单调上涨 / 锯齿 / 首根即持仓 / 穿零 / 含 NaN) 锁定。故这是纯内存优化,
/// **不改变任何指标数值**。
#[derive(Debug, Clone)]
/// 权益曲线的**在线**统计累加器: O(1) 内存。
///
/// 与「物化曲线后单遍遍历」**逐位一致**, 故 [`simulate`] 可省掉 O(T) 的曲线内存。
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
    fn push(&mut self, o: CloseOutcome) {
        self.num_trades += 1;
        self.pnls.push(o.pnl);
        self.fixed_pnls.push(o.fixed_pnl);
        self.total_fixed_pnl += o.fixed_pnl;
        self.pnl_pct_sum += o.pnl_pct;
        self.sum_mfe += o.max_runup;
        self.sum_mae += o.max_drawdown;
        self.sum_hold += o.bars_held as f64;
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
fn reference_curve_stats(curve: &[f64], initial_cash: f64, bars_per_year: f64) -> CurveStats {
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

/// 由**唯一**指标源计算全部 `SummaryMetrics`。
///
/// 签名只收 [`MetricsSource`] —— 落盘产物(`trades` / 曲线)在类型上不可达, 故
/// 指标**不可能**依赖 `--detail` 开关。
fn compute_metrics(out: &MetricsSource, initial_cash: f64, bars_per_year: f64) -> SummaryMetrics {
    let n = out.eq.len();
    if n < 2 {
        return SummaryMetrics::default();
    }

    // 曲线统计全部来自在线累加器(与物化口径逐位一致, 见 CurveAccum 的文档)。
    let st = out.eq.stats(bars_per_year);
    let final_eq = out.eq.last();
    let total_return = st.total_return;
    let max_dd = st.max_dd;
    let pnls = &out.acc.pnls;

    let num_bars = (n - 1) as f64;
    let years = if bars_per_year > 0.0 {
        num_bars / bars_per_year
    } else {
        0.0
    };
    let sharpe = st.sharpe;
    let sortino = st.sortino;
    let volatility = st.volatility;

    let cagr = if bars_per_year > 0.0 && n > 1 {
        let years = num_bars / bars_per_year;
        if years > 0.0 && initial_cash > 0.0 && final_eq > 0.0 {
            (final_eq / initial_cash).powf(1.0 / years) - 1.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let calmar = if max_dd > 0.0 { cagr / max_dd } else { 0.0 };

    let recovery_factor = if max_dd > 0.0 {
        total_return / max_dd
    } else {
        0.0
    };

    let num_trades = out.acc.num_trades;
    // avg_mae / avg_mfe / avg_hold_bars 都进报告列, 故 TradeAccum 同时累计
    // 这三个标量 —— 不必为它们保留整个 Vec<Trade>。
    let mut num_wins = 0u32;
    let mut num_losses = 0u32;
    let mut total_win = 0.0;
    let mut total_loss = 0.0;
    let mut cw = 0u32;
    let mut cl = 0u32;
    let mut max_cw = 0u32;
    let mut max_cl = 0u32;

    let mut pnl_m2 = 0.0;
    let mut pnl_m3 = 0.0;
    let mut pnl_m4 = 0.0;
    let mut runs = 0u32;
    let mut prev_win: Option<bool> = None;

    let nt = num_trades as f64;
    let pnl_mean = if nt > 0.0 {
        pnls.iter().sum::<f64>() / nt
    } else {
        0.0
    };

    for &p in pnls.iter() {
        let cur_win = p > 0.0;
        match prev_win {
            None => runs = 1,
            Some(prev) if prev != cur_win => runs += 1,
            _ => {}
        }
        prev_win = Some(cur_win);

        if cur_win {
            num_wins += 1;
            total_win += p;
            cw += 1;
            cl = 0;
            if cw > max_cw {
                max_cw = cw;
            }
        } else {
            num_losses += 1;
            total_loss += p.abs();
            cl += 1;
            cw = 0;
            if cl > max_cl {
                max_cl = cl;
            }
        }

        let d = p - pnl_mean;
        let d2 = d * d;
        pnl_m2 += d2;
        pnl_m3 += d2 * d;
        pnl_m4 += d2 * d2;
    }

    let avg_win = if num_wins > 0 {
        total_win / num_wins as f64
    } else {
        0.0
    };
    let avg_loss = if num_losses > 0 {
        total_loss / num_losses as f64
    } else {
        0.0
    };
    let profit_factor = if total_loss > 0.0 {
        total_win / total_loss
    } else {
        total_win.max(0.0)
    };
    let expectancy = if num_trades > 0 {
        out.acc.pnl_pct_sum / f64::from(num_trades)
    } else {
        0.0
    };

    let avg_mae = if nt > 0.0 { out.acc.sum_mae / nt } else { 0.0 };
    let avg_mfe = if nt > 0.0 { out.acc.sum_mfe / nt } else { 0.0 };
    let edge_ratio = if avg_mae > 0.0 {
        avg_mfe / avg_mae
    } else {
        0.0
    };
    let avg_hold_bars = if nt > 0.0 { out.acc.sum_hold / nt } else { 0.0 };

    let wl_ratio = if avg_loss > 0.0 {
        avg_win / avg_loss
    } else {
        0.0
    };

    let skewness = if nt > 2.0 && pnl_m2 > 0.0 {
        let pnl_std = (pnl_m2 / (nt - 1.0)).sqrt();
        (pnl_m3 / nt) / pnl_std.powi(3) * ((nt * (nt - 1.0)).sqrt() / (nt - 2.0))
    } else {
        0.0
    };

    let kurtosis = if nt > 3.0 && pnl_m2 > 0.0 {
        let n = nt;
        let m2 = pnl_m2 / n;
        let m4 = pnl_m4 / n;
        ((n + 1.0) * n * (n - 1.0)) / ((n - 2.0) * (n - 3.0)) * (m4 / (m2 * m2))
            - 3.0 * (n - 1.0) * (n - 1.0) / ((n - 2.0) * (n - 3.0))
    } else {
        0.0
    };

    let sqn = if nt > 1.0 && pnl_m2 > 0.0 {
        let pnl_std = (pnl_m2 / (nt - 1.0)).sqrt();
        if pnl_std > 0.0 {
            // SQN = √n · mean/stdev (Van Thorp, "The Risk of Ruin" 1998)。
            // √n 是定义中**有意的**统计显著性因子, 不可封顶: 原实现用 min(n,100)
            // 会让 n=200 与 n=20000 得到相同 SQN, 抹掉"证据量"这一维度。
            // 小样本的可读性由"配对 num_trades 列 + 门槛"解决(Van Thorp 建议 n>=30)。
            pnl_mean / pnl_std * nt.sqrt()
        } else {
            0.0
        }
    } else {
        0.0
    };

    let pf_stability = {
        let half = num_trades as usize / 2;
        if half >= 10 {
            let pf1 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[..half] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let pf2 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[half..] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let (mn, mx) = if pf1 < pf2 { (pf1, pf2) } else { (pf2, pf1) };
            if mx > 0.0 { mn / mx } else { 0.0 }
        } else {
            0.0
        }
    };

    let (z_score, p_value) = if num_trades > 1 && num_wins > 0 && num_losses > 0 {
        let nw = num_wins as f64;
        let nl = num_losses as f64;
        let n = num_trades as f64;
        let e_runs = 2.0 * nw * nl / n + 1.0;
        let var_runs = 2.0 * nw * nl * (2.0 * nw * nl - n) / (n * n * (n - 1.0));
        let std_runs = var_runs.sqrt();
        let z = if std_runs > 0.0 {
            (runs as f64 - e_runs) / std_runs
        } else {
            0.0
        };
        let p = 2.0 * (1.0 - normal_cdf(z.abs()));
        (z, p)
    } else {
        (0.0, 1.0)
    };

    tracing::debug!(
        num_trades,
        total_return_pct = total_return * 100.0,
        sharpe_ratio = sharpe,
        max_drawdown_pct = max_dd * 100.0,
        sqn,
        "metrics computed"
    );

    // fixed 腿统计: 以逐笔 fixed_pnl 与 fixed 盯市曲线为准, 与复利腿同公式可比。
    // 研究报告统一引用 fixed_*; 无后缀的复利口径仅供容量参考。
    let mut fixed_win = 0.0;
    let mut fixed_loss = 0.0;
    // fixed 腿的 win/loss 拆分。非 detailed 路径没有 Trade, 但逐笔 fixed_pnl
    // 可以从 acc 复原 —— 故把 fixed_pnl 一并留在 CloseOutcome 里。
    for &f in out.acc.fixed_pnls.iter() {
        if f > 0.0 {
            fixed_win += f;
        } else {
            fixed_loss += f.abs();
        }
    }
    let fixed_pf = if fixed_loss > 0.0 {
        fixed_win / fixed_loss
    } else {
        fixed_win.max(0.0)
    };
    let fixed_expectancy_bps = if nt > 0.0 {
        out.acc.total_fixed_pnl / nt / initial_cash * 10_000.0
    } else {
        0.0
    };
    let fx_st = out.fx.stats(bars_per_year);
    SummaryMetrics {
        total_return_pct: total_return * 100.0,
        total_return,
        sharpe_ratio: sharpe,
        sortino_ratio: sortino,
        max_drawdown_pct: max_dd * 100.0,
        win_rate: if num_trades > 0 {
            num_wins as f64 / num_trades as f64 * 100.0
        } else {
            0.0
        },
        profit_factor,
        calmar_ratio: calmar,
        num_trades,
        avg_win,
        avg_loss,
        expectancy,
        consecutive_wins: max_cw,
        consecutive_losses: max_cl,
        final_equity: final_eq,
        volatility,
        recovery_factor,
        avg_hold_bars,
        avg_mae,
        avg_mfe,
        edge_ratio,
        wl_ratio,
        skewness,
        kurtosis,
        sqn,
        pf_stability,
        z_score,
        p_value,
        total_fixed_pnl: out.acc.total_fixed_pnl,
        turnover_per_year: if years > 0.0 {
            f64::from(num_trades) / years
        } else {
            0.0
        },
        // 执行口径由 simulate()/落盘层回填, 裸 compute 为旧口径 0。
        delay_bars: 0,
        fee_bps: 0.0,
        slippage_bps: 0.0,
        fixed_total_return: fx_st.total_return,
        fixed_sharpe_ratio: fx_st.sharpe,
        fixed_max_drawdown_pct: fx_st.max_dd * 100.0,
        fixed_expectancy_bps,
        fixed_profit_factor: fixed_pf,
    }
}

fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    sign * y
}

/// 执行一次回测并返回全部指标。
///
/// **不物化权益曲线**(内存 O(1) 而非 O(T))。`--detail` 开关不影响任何指标数值,
/// 详见 [`crate::lib`] 的不变量 2。
#[must_use]
pub fn simulate(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: f64,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> SummaryMetrics {
    let n = close.len().min(signals.len());
    if n < 2 {
        return SummaryMetrics::default();
    }
    // collect_curves = false: 不物化曲线与逐笔 Trade, 统计量在线累计(内存 O(1))。
    let out = scan_trades(close, open, signals, &[], initial_cash, exec, false);
    debug_assert!(
        !out.collect_curves && out.detail.trades.is_empty(),
        "simulate 不得物化落盘产物"
    );
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year);
    summary.delay_bars = exec.delay_u32();
    summary.fee_bps = exec.fee_rate * 10_000.0;
    summary.slippage_bps = exec.slip_rate * 10_000.0;
    summary
}

/// 执行一次回测并额外产出逐笔交易与两条曲线。
///
/// 指标经与 [`simulate`] **同一条**路径计算, 二者逐位一致。
#[must_use]
pub fn simulate_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: f64,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> SimResult {
    let n = close.len().min(signals.len());
    if n < 2 {
        return SimResult {
            summary: SummaryMetrics::default(),
            trades: Vec::new(),
            equity_curve: Vec::new(),
            fixed_curve: Vec::new(),
        };
    }
    // detailed 路径要落盘, 故额外物化逐笔 Trade 与两条曲线。
    // 注意指标仍只经 `out.metrics` 计算 —— 与 `simulate` 完全同源。
    let out = scan_trades(close, open, signals, time, initial_cash, exec, true);
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year);
    summary.delay_bars = exec.delay_u32();
    summary.fee_bps = exec.fee_rate * 10_000.0;
    summary.slippage_bps = exec.slip_rate * 10_000.0;

    let detail = out.detail;
    let equity_curve: Vec<(f64, f64)> = detail
        .equity
        .into_iter()
        .enumerate()
        .map(|(i, eq)| {
            let ts = if i < time.len() {
                time[i] as f64
            } else {
                i as f64
            };
            (ts, eq)
        })
        .collect();

    SimResult {
        summary,
        trades: detail.trades,
        equity_curve,
        fixed_curve: detail.fixed,
    }
}

/// 批量回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
#[must_use]
pub fn simulate_batch(
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: f64,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> Vec<SummaryMetrics> {
    signal_batch
        .par_iter()
        .map(|signals| simulate(close, open, signals, initial_cash, bars_per_year, exec))
        .collect()
}

/// 批量详细回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
#[must_use]
pub fn simulate_batch_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: f64,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> Vec<SimResult> {
    signal_batch
        .par_iter()
        .map(|signals| {
            simulate_detailed(
                time,
                close,
                open,
                signals,
                initial_cash,
                bars_per_year,
                exec,
            )
        })
        .collect()
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    fn make_prices(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 20.0)
            .collect()
    }

    #[test]
    fn test_empty_returns_default() {
        let m = simulate(&[], &[], &[], 1000.0, 252.0, &ExecConfig::default());
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_no_signals_hold_value() {
        let close = vec![100.0; 100];
        let sig = vec![Signal::Hold; 100];
        let m = simulate(&close, &close, &sig, 1000.0, 252.0, &ExecConfig::default());
        assert!((m.total_return_pct).abs() < 1e-10);
    }

    #[test]
    fn test_buy_and_hold() {
        let close: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        let mut sig = vec![Signal::Hold; 100];
        sig[0] = Signal::Buy;
        let m = simulate(&close, &close, &sig, 1000.0, 252.0, &ExecConfig::default());
        assert!(m.total_return_pct > 95.0);
        assert_eq!(m.num_trades, 1);
    }

    #[test]
    fn test_default_exec_fills_at_next_bar_open() {
        // 默认口径 = delay 1 + fill bar 开盘价: bar0 信号在 bar1 开盘成交,
        // 末根强制收盘平仓。`delay_bars` 恒 >= 1(NonZeroUsize), "当根成交"分支
        // 已从引擎层移除, 故不存在与之逐位一致的旧口径可比。
        let close = vec![100.0, 102.0, 101.0, 105.0];
        let open = vec![99.0, 100.5, 102.5, 101.5];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &ExecConfig::default());
        assert_eq!(m.num_trades, 1);
        assert_eq!(m.delay_bars, 1);
        // 进 open[1]=100.5, 出 close[3]=105; 名义 1000/100.5。
        let qty = 1000.0 / 100.5;
        let expect = (105.0 - 100.5) * qty;
        assert!(
            (m.total_fixed_pnl - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
    }

    /// delay_bars = 0(信号当根成交, 前视乐观)已从**引擎层**彻底移除:
    /// 字段是 `NonZeroUsize`, 类型上无法构造, 构造期校验不可能被绕过。
    #[test]
    fn zero_delay_is_rejected_at_every_entry_point() {
        assert_eq!(NonZeroUsize::MIN.get(), 1, "缺省必须是 1 bar");
        assert!(ExecConfig::default().delay() >= 1);
        assert!(ExecConfig::try_from_bps_fill(0, 0.0, 0.0, false).is_err());
        for (d, fee, slip, close) in [
            (0, 0.0, 0.0, false),
            (0, 2.0, 1.0, true),
            (0, -1.0, 0.0, false),
        ] {
            assert!(
                ExecConfig::try_from_bps_fill(d, fee, slip, close).is_err(),
                "delay=0 必须被拒(d={d}, fee={fee}, slip={slip})"
            );
        }
        // from_bps / from_bps_fill 是 try_from 的 infallible 包装, 遇 0 直接 panic。
        let panicked = std::panic::catch_unwind(|| ExecConfig::from_bps(0, 0.0, 0.0)).is_err();
        assert!(panicked, "from_bps(0) 应 panic");
    }

    #[test]
    fn test_delay_one_fills_at_next_open() {
        // delay=1: bar0 多单按 open[1] 成交; bar2 反手空按 open[3] 离场并建仓。
        let close = vec![100.0, 102.0, 101.0, 105.0, 107.0];
        let open = vec![99.0, 101.0, 102.5, 100.5, 106.0];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps(1, 0.0, 0.0);
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &exec);
        assert_eq!(m.num_trades, 2);
        // 多头: 进 101.0 出 100.5 → (100.5-101.0)/101.0*1000。
        let expect_long = (100.5 - 101.0) / 101.0 * 1000.0;
        // 空头: 进 100.5, 末根 107 收盘离场 → (100.5-107)/100.5*1000。
        let expect_short = (100.5 - 107.0) / 100.5 * 1000.0;
        assert!(
            (m.total_fixed_pnl - (expect_long + expect_short)).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
        assert_eq!(m.delay_bars, 1);
    }

    #[test]
    fn test_fill_close_uses_next_bar_close() {
        // delay=1 + fill_close: bar0 多单按 close[1] 成交; bar2 反手空按 close[3] 离场并建仓。
        // 数据与 test_delay_one_fills_at_next_open 同构, 仅 fill 价位 open→close。
        let close = vec![100.0, 102.0, 101.0, 105.0, 107.0];
        let open = vec![99.0, 101.0, 102.5, 100.5, 106.0];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps_fill(1, 0.0, 0.0, true);
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &exec);
        assert_eq!(m.num_trades, 2);
        // 多头: 进 102.0 出 105.0 → (105.0-102.0)/102.0*1000。
        let expect_long = (105.0 - 102.0) / 102.0 * 1000.0;
        // 空头: 进 105.0, 末根 107 收盘离场 → (105.0-107)/105.0*1000。
        let expect_short = (105.0 - 107.0) / 105.0 * 1000.0;
        assert!(
            (m.total_fixed_pnl - (expect_long + expect_short)).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
    }

    #[test]
    fn test_buy_hold_first_close_to_last_close() {
        // 100→110 名义 1000 → +100/+0.1; 不足 2 根或首价非法 → 0。
        let (pnl, ret) = buy_hold_pnl(&[100.0, 105.0, 110.0], 1000.0);
        assert!((pnl - 100.0).abs() < 1e-9, "{pnl}");
        assert!((ret - 0.1).abs() < 1e-12, "{ret}");
        assert_eq!(buy_hold_pnl(&[100.0], 1000.0), (0.0, 0.0));
        assert_eq!(buy_hold_pnl(&[], 1000.0), (0.0, 0.0));
        assert_eq!(buy_hold_pnl(&[0.0, 110.0], 1000.0), (0.0, 0.0));
    }

    #[test]
    fn test_fee_and_slippage_deducted_per_side() {
        // fee 10bp + slip 10bp 双边: 进出各扣一次。用 Close 离场以隔离单笔数学。
        // delay 恒 >= 1: bar0 信号在 bar1 成交, bar1 的 Close 在 bar2 成交。
        let close = vec![100.0, 100.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Close, Signal::Hold];
        let exec = ExecConfig::from_bps(1, 10.0, 10.0);
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &exec);
        assert_eq!(m.num_trades, 1);
        // 进 open[1]*1.001 = 100.1, 出 open[2]*0.999 = 109.89; 名义 1000/100.1;
        // 毛利 (109.89-100.1)*qty - 进出佣金 (100.1+109.89)*qty*0.001。
        let qty = 1000.0 / 100.1;
        let expect = (109.89 - 100.1) * qty - (100.1 + 109.89) * qty * 0.001;
        assert!(
            (m.total_fixed_pnl - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
        assert!((m.fee_bps - 10.0).abs() < 1e-12);
        assert!((m.slippage_bps - 10.0).abs() < 1e-12);
    }

    #[test]
    fn test_trailing_open_beyond_data_skipped() {
        // 末根信号 delay=1 无未来可成交: 不开仓, 交易数为 0。
        let close = vec![100.0, 101.0, 102.0];
        let open = vec![99.0, 100.0, 101.0];
        let sig = vec![Signal::Hold, Signal::Hold, Signal::Buy];
        let exec = ExecConfig::from_bps(1, 0.0, 0.0);
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &exec);
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_fixed_leg_matches_hand_calc() {
        // fixed 腿: 单多 100→110, 名义 1000 → +100, 期望 +1000bp。
        // delay 恒 >= 1: bar0 进(填在 bar1), bar2 的 Close 填在 bar3。
        let close = vec![100.0, 100.0, 110.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Close, Signal::Hold];
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &ExecConfig::default());
        assert_eq!(m.num_trades, 1);
        assert!(
            (m.fixed_total_return - 0.1).abs() < 1e-12,
            "{}",
            m.fixed_total_return
        );
        assert!(
            (m.fixed_expectancy_bps - 1000.0).abs() < 1e-9,
            "{}",
            m.fixed_expectancy_bps
        );
        assert!(
            (m.fixed_profit_factor - 100.0).abs() < 1e-9,
            "{}",
            m.fixed_profit_factor
        );
        assert!(m.fixed_sharpe_ratio > 0.0);
    }

    #[test]
    fn test_turnover_per_year_counts_round_trips_annualized() {
        let close = make_prices(200);
        let mut sig = vec![Signal::Hold; 200];
        sig[0] = Signal::Buy;
        sig[50] = Signal::Sell;
        sig[100] = Signal::Buy;
        sig[150] = Signal::Sell;

        let m = simulate(
            &close,
            &close,
            &sig,
            10_000.0,
            252.0,
            &ExecConfig::default(),
        );
        assert_eq!(m.num_trades, 4);
        // 年数 = 199 根 bar / 252 bar 每年
        let expected = 4.0 / (199.0 / 252.0);
        assert!(
            (m.turnover_per_year - expected).abs() < 1e-9,
            "{}",
            m.turnover_per_year
        );
    }

    #[test]
    fn test_turnover_per_year_is_zero_without_trades() {
        let close = make_prices(200);
        let m = simulate(
            &close,
            &close,
            &[Signal::Hold; 200],
            10_000.0,
            252.0,
            &ExecConfig::default(),
        );
        assert_eq!(m.turnover_per_year, 0.0);
    }

    #[test]
    fn test_simulate_batch_matches_single() {
        let close = make_prices(200);
        let sig1 = vec![Signal::Hold; 200];
        let mut sig2 = vec![Signal::Hold; 200];
        sig2[0] = Signal::Buy;

        let batch = vec![sig1.clone(), sig2.clone()];
        let exec = ExecConfig::default();
        let results = simulate_batch(&close, &close, &batch, 10_000.0, 252.0, &exec);
        let r1 = simulate(&close, &close, &sig1, 10_000.0, 252.0, &exec);
        let r2 = simulate(&close, &close, &sig2, 10_000.0, 252.0, &exec);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], r1);
        assert_eq!(results[1], r2);
    }

    /// [`CurveAccum`] 与「物化曲线后单遍遍历」的**逐位一致**证明。
    ///
    /// 覆盖 5 类曲线形态, 每类同时锁定 len / last / 全部 5 个统计量:
    /// - 单调上涨: 无回撤、下行样本为 0(sortino 分母退化)
    /// - 锯齿: 有回撤、有下行样本(sortino 与 sharpe 分母不同 → 检验 ddof 约定)
    /// - 首根即非初始值: 检验「首点不参与 peak/max_dd」的口径
    /// - 穿零(空头浮空): 检验 [`log_ret`] 的定义域保护, 且 sortino 有限非 NaN
    /// - 含 NaN: 检验守卫退化为 0 而非污染 NaN
    #[test]
    fn test_curve_accum_is_bit_identical_to_materialized_reference() {
        let cases: Vec<(&str, Vec<f64>)> = vec![
            ("单调上涨", (0..64).map(|i| 1000.0 + i as f64).collect()),
            (
                "锯齿",
                (0..64)
                    .map(|i| {
                        if i % 2 == 0 {
                            1000.0 + i as f64
                        } else {
                            900.0 - i as f64
                        }
                    })
                    .collect(),
            ),
            ("首根即持仓(首点 != initial)", {
                let mut v = vec![1234.5];
                v.extend((1..64).map(|i| 1000.0 + i as f64));
                v
            }),
            (
                "穿零(空头浮空)",
                (0..64).map(|i| 1000.0 - i as f64 * 40.0).collect(),
            ),
            ("含 NaN", {
                let mut v: Vec<f64> = (0..64)
                    .map(|i| 1000.0 + (i as f64 * 3.7).sin() * 50.0)
                    .collect();
                v[10] = f64::NAN;
                v
            }),
        ];

        for (name, curve) in cases {
            let initial = curve[0];
            let mut acc = CurveAccum::new(initial);
            for &eq in &curve {
                acc.push(eq);
            }
            let got = acc.stats(365.0);
            let want = reference_curve_stats(&curve, initial, 365.0);

            assert_eq!(acc.len(), curve.len(), "{name}: len");
            assert_eq!(
                acc.last().to_bits(),
                curve[curve.len() - 1].to_bits(),
                "{name}: last"
            );
            assert_eq!(
                got.sharpe.to_bits(),
                want.sharpe.to_bits(),
                "{name}: sharpe"
            );
            assert_eq!(
                got.sortino.to_bits(),
                want.sortino.to_bits(),
                "{name}: sortino"
            );
            assert_eq!(
                got.max_dd.to_bits(),
                want.max_dd.to_bits(),
                "{name}: max_dd"
            );
            assert_eq!(
                got.volatility.to_bits(),
                want.volatility.to_bits(),
                "{name}: volatility"
            );
            assert_eq!(
                got.total_return.to_bits(),
                want.total_return.to_bits(),
                "{name}: total_return"
            );
            for (k, v) in [
                ("sharpe", got.sharpe),
                ("sortino", got.sortino),
                ("max_dd", got.max_dd),
                ("volatility", got.volatility),
                ("total_return", got.total_return),
            ] {
                assert!(v.is_finite(), "{name}: {k} = {v} 非有限");
            }
        }
    }

    /// [`simulate`] 必须**不物化**落盘产物(内存 O(1) 而非 O(T)); 这是 O2 的全部意义。
    #[test]
    fn test_simulate_does_not_materialize_equity_curve() {
        let close = make_prices(100_000);
        let mut sig = vec![Signal::Hold; 100_000];
        sig[0] = Signal::Buy;
        let exec = ExecConfig::default();
        let m = simulate(&close, &close, &sig, 10_000.0, 365.0, &exec);
        let d = simulate_detailed(&[], &close, &close, &sig, 10_000.0, 365.0, &exec);
        assert_eq!(d.equity_curve.len(), 100_000, "detailed 必须物化曲线");
        assert_eq!(d.fixed_curve.len(), 100_000);

        // Q2 结构性保证: `scan_trades` 在 collect_curves=false 时不得产出任何
        // DetailArtifacts(逐笔 Trade + 两条曲线)。类型上由 Option 表达, 此处
        // 在 debug 构建下由 debug_assert! 复核。
        let out = scan_trades(&close, &close, &sig, &[], 10_000.0, &exec, false);
        assert!(
            !out.collect_curves && out.detail.trades.is_empty(),
            "非 detailed 路径不得物化落盘产物"
        );
        for f in [
            m.sharpe_ratio,
            m.sortino_ratio,
            m.max_drawdown_pct,
            m.volatility,
            m.fixed_sharpe_ratio,
            m.fixed_max_drawdown_pct,
        ] {
            assert!(f.is_finite(), "指标非有限: {f}");
        }
        assert_eq!(m.sharpe_ratio.to_bits(), d.summary.sharpe_ratio.to_bits());
        assert_eq!(m.sortino_ratio.to_bits(), d.summary.sortino_ratio.to_bits());
        assert_eq!(
            m.max_drawdown_pct.to_bits(),
            d.summary.max_drawdown_pct.to_bits()
        );
        assert_eq!(m.volatility.to_bits(), d.summary.volatility.to_bits());
        assert_eq!(
            m.fixed_sharpe_ratio.to_bits(),
            d.summary.fixed_sharpe_ratio.to_bits()
        );
    }

    /// 空头浮空(权益穿零)不得污染统计量: 不得出现 NaN, volatility 不得静默归零。
    #[test]
    fn test_short_blowup_yields_finite_nonzero_ratios() {
        let n = 200;
        let close: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 1.5).collect();
        let mut sig = vec![Signal::Hold; n];
        sig[0] = Signal::Sell; // 全程空头, 权益必然穿零
        let m = simulate(&close, &close, &sig, 1000.0, 8760.0, &ExecConfig::default());
        assert!(m.final_equity < 0.0, "该用例应确实穿零: {}", m.final_equity);
        for (k, v) in [
            ("sharpe", m.sharpe_ratio),
            ("sortino", m.sortino_ratio),
            ("volatility", m.volatility),
            ("max_dd", m.max_drawdown_pct),
        ] {
            assert!(v.is_finite(), "{k} = {v} 非有限(权益穿零被污染)");
        }
        assert!(
            m.volatility > 0.0,
            "volatility 被静默归零(比 NaN 更危险: 会被读成零波动低风险): {}",
            m.volatility
        );
    }

    /// sortino 的分母必须是**下行 bar 数**(ddof=1), 与 sharpe 同自由度。
    /// 若分母误用全部 bar 数, "下跌集中在少数 bar"的曲线会得到明显更小的 sortino。
    #[test]
    fn test_sortino_uses_downside_count_as_denominator() {
        let mut curve = vec![1000.0f64];
        for i in 0..10 {
            curve.push(1000.0 * 1.01f64.powi(i + 1));
        }
        curve.push(900.0);
        curve.push(800.0);
        let mut acc = CurveAccum::new(curve[0]);
        for &c in &curve {
            acc.push(c);
        }
        let st = acc.stats(1.0);

        let rets: Vec<f64> = bar_log_returns(&curve).collect();
        let down_sq: f64 = rets.iter().filter(|&&r| r < 0.0).map(|r| r * r).sum();
        let n_down = rets.iter().filter(|&&r| r < 0.0).count() as f64;
        let mut mean = 0.0;
        for (i, r) in rets.iter().enumerate() {
            mean += (r - mean) / (i + 1) as f64;
        }
        let expect = mean / (down_sq / (n_down - 1.0)).sqrt();
        assert!(
            (st.sortino - expect).abs() < 1e-12,
            "sortino {}(应={expect}) 分母用了错误的 bar 数",
            st.sortino
        );
        let wrong = mean / (down_sq / rets.len() as f64).sqrt();
        assert!(
            (st.sortino - wrong).abs() > 1e-6,
            "两种口径碰巧相同, 该用例失去区分力"
        );
    }

    /// SQN = √n · mean/stdev (Van Thorp), **不封顶**; `√n` 是定义中的统计显著性
    /// 因子, 封顶会抹掉"证据量"这一维度。小样本可读性由 `num_trades >= 30`
    /// 门槛解决(见 `sqn` 字段文档)。
    #[test]
    fn test_sqn_scales_with_full_sample_size() {
        // 每笔的**相对**收益固定按 [+r, +r, -r] 循环, 故 mean/stdev 与笔数无关,
        // SQN 的变化只能来自定义里的 √n 因子。
        let r = 0.001f64;
        let mk = |nt: u32| {
            let n = nt as usize * 3 + 4;
            let mut open = vec![100.0f64; n];
            // delay=1: 3k 处 Buy → 3k+1 开盘成交; 3k+2 处 Close → 3k+3 开盘成交。
            for k in 0..nt as usize {
                let entry = open[3 * k + 1];
                let step = if k % 3 == 2 { -r } else { r };
                open[3 * k + 3] = entry * (1.0 + step);
                if k + 1 < nt as usize {
                    open[3 * (k + 1) + 1] = open[3 * k + 3];
                }
            }
            let mut sig = vec![Signal::Hold; n];
            for k in 0..nt as usize {
                sig[3 * k] = Signal::Buy;
                sig[3 * k + 2] = Signal::Close;
            }
            simulate(&open, &open, &sig, 10_000.0, 252.0, &ExecConfig::default())
        };
        let a = mk(30);
        let b = mk(300);
        assert_eq!(a.num_trades, 30);
        assert_eq!(b.num_trades, 300);
        assert!(a.sqn > 0.0 && b.sqn > 0.0, "用例应产生正 SQN");
        let ratio = b.sqn / a.sqn;
        assert!(
            (2.0..5.0).contains(&ratio),
            "SQN 未随 √n 增长(a={}, b={}, ratio={ratio}); 疑似被封顶",
            a.sqn,
            b.sqn
        );
    }

    /// [`log_ret`] 必须在**两侧**都做定义域保护。只查 `prev` 不够: 首根即浮空
    /// 时 `prev` 仍为正而 `curr/prev < 0`, `ln()` 返回 NaN。
    #[test]
    fn test_bar_log_returns_guards_nonpositive_equity() {
        assert_eq!(bar_log_returns(&[100.0, -50.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[100.0, 0.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[-50.0, 100.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[f64::NAN, 100.0]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[100.0, f64::NAN]).next(), Some(0.0));
        assert_eq!(bar_log_returns(&[f64::INFINITY, 100.0]).next(), Some(0.0));
        let r = bar_log_returns(&[100.0, 110.0]).next().unwrap();
        assert!((r - 1.1f64.ln()).abs() < 1e-15);
        for &x in &[100.0, -1.0, 0.0, f64::NAN, 1e-300] {
            assert!(bar_log_returns(&[x, x]).all(|r| r.is_finite()));
        }
    }

    /// L9 / Q2: 非 detailed 路径不物化落盘产物, 但**指标必须逐位一致**。
    ///
    /// 现在这不只是"测出来一致", 而是**类型上不可能不一致**: `compute_metrics`
    /// 的签名收 `&MetricsSource`, 而 `MetricsSource` 里没有 `trades` / 曲线字段 ——
    /// 指标在编译期就看不到 `--detail` 的产物。两者即便未来实现分叉, 也无法让
    /// 指标依赖 detail 侧的数据。
    ///
    /// 本测试锁住"当前实现确实逐位一致", 防止有人绕过 `&MetricsSource` 重接指标源。
    #[test]
    fn non_detailed_metrics_match_detailed_bit_for_bit() {
        let exec = ExecConfig::try_from_bps_fill(1, 2.0, 1.0, false).unwrap();
        for (name, close) in [
            ("单调上涨", make_prices(300)),
            ("锯齿", {
                let mut c = make_prices(300);
                for (i, v) in c.iter_mut().enumerate() {
                    *v *= if i % 2 == 0 { 1.0 } else { 0.99 };
                }
                c
            }),
        ] {
            let mut sig = vec![Signal::Hold; close.len()];
            // 多空交替 -> 同时覆盖 Long / Short / 平仓三种路径
            for i in (0..close.len()).step_by(7) {
                sig[i] = if (i / 7) % 2 == 0 {
                    Signal::Buy
                } else {
                    Signal::Sell
                };
            }
            sig[2] = Signal::Close;
            let a = simulate(&close, &close, &sig, 10_000.0, 252.0, &exec);
            let b = simulate_detailed(&[], &close, &close, &sig, 10_000.0, 252.0, &exec);
            assert_eq!(a.num_trades, b.summary.num_trades, "{name}: num_trades");
            assert_eq!(b.trades.len(), a.num_trades as usize, "{name}: trades 长度");
            for (k, x) in [
                ("total_return", a.total_return),
                ("sharpe", a.sharpe_ratio),
                ("sortino", a.sortino_ratio),
                ("max_dd", a.max_drawdown_pct),
                ("volatility", a.volatility),
                ("profit_factor", a.profit_factor),
                ("win_rate", a.win_rate),
                ("avg_mae", a.avg_mae),
                ("avg_mfe", a.avg_mfe),
                ("avg_hold_bars", a.avg_hold_bars),
                ("edge_ratio", a.edge_ratio),
                ("sqn", a.sqn),
                ("skewness", a.skewness),
                ("kurtosis", a.kurtosis),
                ("pf_stability", a.pf_stability),
                ("total_fixed_pnl", a.total_fixed_pnl),
                ("fixed_expectancy_bps", a.fixed_expectancy_bps),
                ("fixed_profit_factor", a.fixed_profit_factor),
                ("fixed_sharpe", a.fixed_sharpe_ratio),
                ("fixed_mdd", a.fixed_max_drawdown_pct),
                ("fixed_return", a.fixed_total_return),
            ] {
                let y = match k {
                    "total_return" => b.summary.total_return,
                    "sharpe" => b.summary.sharpe_ratio,
                    "sortino" => b.summary.sortino_ratio,
                    "max_dd" => b.summary.max_drawdown_pct,
                    "volatility" => b.summary.volatility,
                    "profit_factor" => b.summary.profit_factor,
                    "win_rate" => b.summary.win_rate,
                    "avg_mae" => b.summary.avg_mae,
                    "avg_mfe" => b.summary.avg_mfe,
                    "avg_hold_bars" => b.summary.avg_hold_bars,
                    "edge_ratio" => b.summary.edge_ratio,
                    "sqn" => b.summary.sqn,
                    "skewness" => b.summary.skewness,
                    "kurtosis" => b.summary.kurtosis,
                    "pf_stability" => b.summary.pf_stability,
                    "total_fixed_pnl" => b.summary.total_fixed_pnl,
                    "fixed_expectancy_bps" => b.summary.fixed_expectancy_bps,
                    "fixed_profit_factor" => b.summary.fixed_profit_factor,
                    "fixed_sharpe" => b.summary.fixed_sharpe_ratio,
                    "fixed_mdd" => b.summary.fixed_max_drawdown_pct,
                    _ => b.summary.fixed_total_return,
                };
                assert_eq!(
                    x.to_bits(),
                    y.to_bits(),
                    "{name}: {k} 两条路径不一致 —— simulate={x}, simulate_detailed={y}"
                );
            }
        }
    }

    /// `exit_reason` 的取值集合必须封闭(落盘层按它做字典序统计)。
    #[test]
    fn exit_reason_values_are_closed_and_wrong() {
        assert_eq!(ExitReason::Signal.as_str(), "signal");
        assert_eq!(ExitReason::EndOfData.as_str(), "end_of_data");
        let n = 60;
        let close = make_prices(n);
        let mut sig = vec![Signal::Hold; n];
        sig[0] = Signal::Buy;
        // 末根仍持仓 -> 必须标 end_of_data; 中间反手 -> signal
        sig[10] = Signal::Sell;
        // 末根不再给平仓信号 -> 数据末尾强制平仓
        let d = simulate_detailed(
            &[],
            &close,
            &close,
            &sig,
            10_000.0,
            252.0,
            &ExecConfig::default(),
        );
        let reasons: Vec<&str> = d.trades.iter().map(|t| t.exit_reason).collect();
        assert_eq!(reasons, vec!["signal", "end_of_data"]);
        // 非 detailed 路径不得构造 Trade: exit_reason 已是 &'static str, 无堆分配
        for t in &d.trades {
            assert!(
                matches!(t.exit_reason, "signal" | "end_of_data"),
                "未知 exit_reason: {}",
                t.exit_reason
            );
        }
    }
}
