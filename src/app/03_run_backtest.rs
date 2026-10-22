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
    /// 盈利笔数占**已决笔数**(盈利 + 亏损, 不含保本)的比例(百分数)。
    ///
    /// 保本(`pnl == 0`)既不算赢也不算亏, 故不进入分母 —— 否则"零盈亏"
    /// 会被读成一次失败交易, 系统性拉低胜率。
    pub win_rate: f64,
    /// 总盈利 / 总亏损。无亏损时取总盈利。
    pub profit_factor: f64,
    /// CAGR / 最大回撤。无回撤时为 0。
    pub calmar_ratio: f64,
    /// 已平仓交易笔数(往返各计一次)。
    pub num_trades: u32,
    /// 盈利笔的平均 pnl(货币; 定点十进制)。
    pub avg_win: crate::money::Amount,
    /// 亏损笔的平均亏损**绝对值**(货币, 正数; 定点十进制)。
    pub avg_loss: crate::money::Amount,
    /// 逐笔 `pnl_pct` 的均值(小数)。
    ///
    /// **毛口径**: `pnl_pct` 由已含滑点的成交价算出, 但**不含佣金**。
    /// 与 [`Self::fixed_expectancy_bps`] 的差就是佣金拖累。
    pub expectancy: f64,
    /// 最长连续盈利笔数。
    ///
    /// 保本笔**不计入也不打断**连续段(它不构成一次盈利或亏损)。
    pub consecutive_wins: u32,
    /// 最长连亏笔数。
    ///
    /// 保本笔**不计入也不打断**连续段: 否则"零盈亏"会被当作一次亏损,
    /// 把连亏段人为拉长。
    pub consecutive_losses: u32,
    /// 末根权益(货币; 定点十进制)。空头浮空时可为负。
    pub final_equity: crate::money::Amount,
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
    /// 逐笔 pnl 的样本偏度(Fisher-Pearson 小样本校正; 正态=0)。
    pub skewness: f64,
    /// 逐笔 pnl 的超额峰度(Fisher 小样本校正; 正态=0; 仅 `num_trades > 3` 时计算)。
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
    /// fixed 腿(固定名义、不复利)总盈亏(货币; 定点十进制)。
    pub total_fixed_pnl: crate::money::Amount,
    /// 年化成交笔数(往返次数)。信号抖动的主要预警指标:
    /// 平均持仓不足 5 根 bar 时, 该值会高到成本必然吞掉全部毛利。
    pub turnover_per_year: f64,
    /// 执行口径: 信号到成交的 bar 间隔(恒 ≥1)。
    pub delay_bars: u32,
    /// 执行口径: 单边佣金(bp)。
    pub fee_bps: f64,
    /// 执行口径: 单边逆向滑点(bp)。
    pub slippage_bps: f64,
    /// fixed 腿总收益率(小数, 固定名义不复利)。研究报告引用此值, 而非复利 total_return。
    pub fixed_total_return: f64,
    /// fixed 腿年化夏普(与 sharpe_ratio 同公式, 曲线为 fixed 盯市)。
    pub fixed_sharpe_ratio: f64,
    /// fixed 腿最大回撤(百分数)。
    pub fixed_max_drawdown_pct: f64,
    /// **净期望**(bp, 以初始资金计): 固定名义腿的逐笔净盈亏均值,
    /// **含双边佣金与滑点**。与 [`Self::expectancy`] 相减即成本拖累
    /// (约等于两倍单边费率)。C6 成本分析的直接输入。
    pub fixed_expectancy_bps: f64,
    /// fixed 腿 profit factor。
    pub fixed_profit_factor: f64,
}

impl SummaryMetrics {
    /// 回填执行口径字段(`delay_bars` / `fee_bps` / `slippage_bps`)。
    ///
    /// 这三列描述的是**本次运行的执行假设**, 不是从交易序列算出的指标。
    /// 引擎存在多条返回路径(正常计算、数据不足提前返回、报告层构造的
    /// `invalid` 行), 若各写一份就容易出现"同一列有的行是 0、有的行是真实值"。
    /// 故收敛到这一个方法: 凡是产出 `SummaryMetrics` 的路径都必须调用它。
    pub fn apply_exec(&mut self, exec: &ExecConfig) {
        self.delay_bars = exec.delay_u32();
        self.fee_bps = exec.fee_rate * 10_000.0;
        self.slippage_bps = exec.slip_rate * 10_000.0;
    }
}

/// 成交执行假设。`delay_bars = 1` 为推荐口径(信号次根 fill bar 成交,
/// `fill_close=false` 取开盘, `fill_close=true` 取收盘)。费率按单边名义扣除。
///
/// **`delay_bars` 恒 ≥ 1**: 信号次根 fill bar 成交。`delay_bars = 0`(信号当根收盘成交)
/// 属前视乐观口径, 已在**引擎层彻底移除** —— 字段是 [`NonZeroUsize`], 类型上无法
/// 构造 0。费率按单边名义扣除。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecConfig {
    /// 信号到成交的 bar 间隔, 恒 ≥ 1(类型保证)。1 = 次根 fill bar。
    pub delay_bars: NonZeroUsize,
    /// 单边佣金率(= bps / 10000)。**必须 >= 0 且有限**; 构造期与引擎入口
    /// ([`simulate`] / [`simulate_detailed`]) 都会校验。
    pub fee_rate: f64,
    /// 单边逆向滑点率(= bps / 10000)。约束同 [`Self::fee_rate`]。
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
    /// 构造执行配置。`delay_bars = 0`、负/非有限的费率或滑点、或换算回 bps
    /// 会溢出的费率直接拒绝。
    ///
    /// # Errors
    /// `delay_bars == 0`、`fee_bps`/`slippage_bps` 为负或 `NaN`/`Inf`、或
    /// 派生 bps 非有限时返回 [`AppError::Msg`](crate::error::AppError::Msg)。
    ///
    /// 配置路径在反序列化期已用 newtype 保证非负有限; 这里再校验一次, 是因为
    /// [`ExecConfig`] 是**公开的引擎 API**, 调用方可以绕过
    /// [`crate::config::BacktestConfig`] 直接构造。负费率会让每笔交易"倒赚钱",
    /// `NaN` 会污染整条权益曲线 —— 都属于必须在入口拒绝而不是静默传播的输入。
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
        if !fee_bps.is_finite() || fee_bps < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "fee_bps 必须 >= 0 且为有限值, 当前为 {fee_bps}"
            )));
        }
        if !slippage_bps.is_finite() || slippage_bps < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "slippage_bps 必须 >= 0 且为有限值, 当前为 {slippage_bps}"
            )));
        }
        let config = Self {
            delay_bars,
            fee_rate: fee_bps / 10_000.0,
            slip_rate: slippage_bps / 10_000.0,
            fill_close,
        };
        // 费率本身有限不代表 `rate * 10_000` 有限; 后者会写进报告的 bps 列。
        config.validate_rates()?;
        Ok(config)
    }

    /// [`Self::try_from_bps_fill`] 的 infallible 版本(固定 `fill_close = false`)。
    ///
    /// # Panics
    /// `delay_bars == 0`、负/非有限的费率或滑点时 panic。
    ///
    /// 这个 `expect` 是**故意的"bug 探测器"**(`err-expect-bugs-only`):
    /// 这些值不是用户输入错误 —— 配置路径在**反序列化期**就拒了
    /// (`BacktestConfig::try_from`), 能走到这里的非法值一定是代码写错了,
    /// 应当立刻炸掉而不是返回一个"前视乐观/倒赚手续费"的执行口径。
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn from_bps_fill(
        delay_bars: usize,
        fee_bps: f64,
        slippage_bps: f64,
        fill_close: bool,
    ) -> Self {
        Self::try_from_bps_fill(delay_bars, fee_bps, slippage_bps, fill_close)
            .expect("执行参数必须合法: delay>=1 且 fee/slip 非负有限")
    }

    /// 引擎入口的最终校验: 即使调用方绕过构造函数直接写公开字段, 也不能把
    /// 负费率 / `NaN` 带进账本。
    fn validate_rates(&self) -> crate::error::Result<()> {
        if !self.fee_rate.is_finite() || self.fee_rate < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "fee_rate 必须 >= 0 且为有限值, 当前为 {}",
                self.fee_rate
            )));
        }
        if !self.slip_rate.is_finite() || self.slip_rate < 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "slip_rate 必须 >= 0 且为有限值, 当前为 {}",
                self.slip_rate
            )));
        }
        // `apply_exec` 会写回 `rate * 10_000`; 该派生值也必须是有限数。
        if !(self.fee_rate * 10_000.0).is_finite() {
            return Err(crate::error::AppError::Msg(
                "fee_rate 过大: 换算回 fee_bps 时溢出".to_string(),
            ));
        }
        if !(self.slip_rate * 10_000.0).is_finite() {
            return Err(crate::error::AppError::Msg(
                "slip_rate 过大: 换算回 slippage_bps 时溢出".to_string(),
            ));
        }
        Ok(())
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
    /// 复利腿持仓数量(金额量纲, 定点十进制)。
    pub qty: crate::money::Amount,
    /// 复利腿盈亏(货币, 已扣双边佣金; 定点十进制)。
    pub pnl: crate::money::Amount,
    /// 相对开仓价的收益率(小数)。
    pub pnl_pct: f64,
    /// fixed 腿盈亏(固定名义, 已扣双边佣金; 定点十进制)。
    pub fixed_pnl: crate::money::Amount,
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
    /// 持仓数量(金额量纲, 定点十进制)。
    pos_qty: crate::money::Amount,
    pos_entry: f64,
    pos_entry_time: i64,
    pos_entry_bar: usize,
    exit_price: f64,
    exit_time: i64,
    exit_bar: usize,
    /// 初始资金(定点十进制)。
    initial_cash: crate::money::Amount,
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
///
/// `pnl` 是**金额**(定点十进制), `return` 是**比率**(无量纲 ⇒ `f64`)。
pub fn buy_hold_pnl(
    close: &[f64],
    nominal: crate::money::Amount,
) -> crate::error::Result<(crate::money::Amount, f64)> {
    if close.len() < 2 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let (first, last) = (close[0], close[close.len() - 1]);
    if !first.is_finite() || !last.is_finite() || first <= 0.0 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let ret = last / first - 1.0;
    // 金额 = 名义 × 收益率; 名义是金额, 收益率是无量纲比率。
    let pnl = nominal.mul_ratio(ret)?;
    Ok((pnl, ret))
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

/// fill bar `bar`、订单方向 `side` 的成交价。
///
/// `fill_close=false`(默认)取开盘, `true` 取收盘; 开盘缺失时退回收盘。
/// 成交价非有限或非正时返回 `None` —— 调用方据此**放弃本 bar 的成交**,
/// 持仓状态保持不变, 而不是拿会污染全部指标的 NaN/0 价格去结算。
///
/// 成交只发生在信号 bar + `delay_bars` 的 fill bar 上(`delay_bars` 恒 ≥ 1,
/// 由 `NonZeroUsize` 保证), 故调用方必须先排队、到期后再调用本函数。
#[inline]
fn fill_at(bar: usize, side: Side, close: &[f64], open: &[f64], exec: &ExecConfig) -> Option<f64> {
    let base = if exec.fill_close {
        close[bar]
    } else {
        match open.get(bar).copied() {
            Some(p) => p,
            None => {
                // 生产路径不会走到这里: `KlineColumns` 强制各列等长。只有直接
                // 调公共 API 且 `open` 偏短时才会触发 —— 退回收盘价并留一条
                // **一次性** debug 记录, 避免静默改变成交价假设。
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| {
                    tracing::debug!(bar, "open 缺失, 成交价回退到 close(仅提示一次)");
                });
                close[bar]
            }
        }
    };
    let price = fill_price(side, base, exec.slip_rate);
    valid_fill_price(price).then_some(price)
}

/// 一笔平仓的**标量**结果。
///
/// 与 [`Trade`] 分离的原因: `simulate()`(非 detailed 路径)只需要标量来算统计量,
/// 根本不需要 `Trade`。原先两者被绑在一起, 导致每个参数求值都要为每笔交易
/// 构造一个 `Trade` —— 其中 `exit_reason: String` 是**堆分配**。参数网格是串行
/// 外循环(策略层并行会 OOM, 见 README), 这笔开销直接乘以总交易笔数。
#[derive(Debug, Clone, Copy)]
struct CloseOutcome {
    /// 现金变动(定点十进制)。
    cash_delta: crate::money::Amount,
    /// 本笔盈亏(定点十进制)。
    pnl: crate::money::Amount,
    /// 本笔收益率(**比率**, 无量纲 ⇒ 仍是 `f64`)。
    pnl_pct: f64,
    /// fixed 腿本笔盈亏(定点十进制)。
    fixed_pnl: crate::money::Amount,
    bars_held: i64,
    max_runup: f64,
    max_drawdown: f64,
}

/// 已平仓交易的累计量。非 detailed 路径只需要它, 不需要保留逐笔 [`Trade`]。
#[derive(Debug, Default)]
struct TradeAccum {
    num_trades: u32,
    /// 逐笔 `pnl`: SQN / skew / kurtosis / pf-stability 的分半统计都要用它。
    ///
    /// **统计序列用 `f64`**: 这些是**统计量**(偏度/峰度/SQN), 本身是连续量,
    /// 不是"账上的钱"。账上的钱(累计 `total_fixed_pnl`)是 `Amount`。
    pnls: Vec<f64>,
    /// 逐笔 `fixed_pnl`: fixed 腿的 win/loss 拆分与 expectancy(统计序列)。
    fixed_pnls: Vec<f64>,
    /// 累计 fixed 腿盈亏(**金额**, 定点十进制)。
    total_fixed_pnl: crate::money::Amount,
    /// 盈利笔累计盈亏(**金额**, 定点十进制)。
    total_win: crate::money::Amount,
    /// 亏损笔累计亏损绝对值(**金额**, 定点十进制)。
    total_loss: crate::money::Amount,
    /// 逐笔 `pnl_pct` 之和(仅用于 avg pnl_pct; 无需保留整个序列)。
    pnl_pct_sum: f64,
    /// MFE / MAE / 持仓 bar 数之和。**报告列 `avg_mfe`/`avg_mae`/`avg_hold_bars`
    /// 需要它们**, 故非 detailed 路径也必须累计(只需三个标量, 不必留逐笔 Trade)。
    sum_mfe: f64,
    sum_mae: f64,
    sum_hold: f64,
}

/// 计算一次平仓的**钱**与收益率。
///
/// 钱的部分全部走 [`Amount`](crate::money::Amount)(定点十进制): 佣金、
/// 现金变动、单笔盈亏、fixed 腿盈亏。价格与费率仍是 `f64`(价格是连续量,
/// 费率无量纲), 但**钱**本身不再用 `f64` 承载。
///
/// # Errors
/// 金额运算溢出、或除以非法价格时返回 [`AppError`](crate::error::AppError)。
fn close_outcome(args: CloseArgs) -> crate::error::Result<CloseOutcome> {
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
    // 开仓佣金在建仓时已从 `cash` 扣掉, 但**逐笔 `pnl` 必须同样扣掉它** ——
    // 否则 `sum(trade.pnl) != final_equity - initial_cash`, win/loss/PF/SQN
    // 等一切基于逐笔 pnl 的统计都会系统性偏乐观。
    let entry_fee = pos_qty.mul_price(pos_entry)?.mul_ratio(fee_rate)?;
    let exit_fee = pos_qty.mul_price(exit_price)?.mul_ratio(fee_rate)?;
    let (cash_delta, pnl) = if cur_pos == Side::Long {
        (
            pos_qty.mul_price(exit_price)?.minus(exit_fee)?,
            pos_qty
                .mul_price(exit_price - pos_entry)?
                .minus(entry_fee)?
                .minus(exit_fee)?,
        )
    } else {
        (
            pos_qty.mul_price(exit_price)?.negated().minus(exit_fee)?,
            pos_qty
                .mul_price(pos_entry - exit_price)?
                .minus(entry_fee)?
                .minus(exit_fee)?,
        )
    };
    let fixed_qty = initial_cash.div_price(pos_entry)?;
    let entry_fee_fixed = fixed_qty.mul_price(pos_entry)?.mul_ratio(fee_rate)?;
    let exit_fee_fixed = fixed_qty.mul_price(exit_price)?.mul_ratio(fee_rate)?;
    let fixed_pnl = if cur_pos == Side::Long {
        fixed_qty
            .mul_price(exit_price - pos_entry)?
            .minus(entry_fee_fixed)?
            .minus(exit_fee_fixed)?
    } else {
        fixed_qty
            .mul_price(pos_entry - exit_price)?
            .minus(entry_fee_fixed)?
            .minus(exit_fee_fixed)?
    };
    // 收益率是**比率**(无量纲)⇒ 仍是 `f64`。
    //
    // **有意与 `pnl` 不同口径**: `pnl`/`fixed_pnl` 是**净额**(已扣双边佣金),
    // 而 `pnl_pct` 是**毛比率**(成交价已含滑点, 不再扣佣金)。二者并存是为了
    // 让 `expectancy` 保持"策略边际"语义、与费率配置解耦; 需要净期望时用
    // `SummaryMetrics::fixed_expectancy_bps`。
    let pnl_pct = if cur_pos == Side::Long {
        (exit_price - pos_entry) / pos_entry
    } else {
        (pos_entry - exit_price) / pos_entry
    };
    Ok(CloseOutcome {
        cash_delta,
        pnl,
        pnl_pct,
        fixed_pnl,
        bars_held: (exit_bar as i64) - (pos_entry_bar as i64),
        max_runup: trade_mfe,
        max_drawdown: trade_mae,
    })
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
    /// 末根权益的**精确定点值**(不是从 f64 曲线回造的近似值)。
    ///
    /// 报告里的 `final_equity` 与 `total_return` 必须由账本直接给出: 先转成
    /// `f64` 再 `Amount::from_f64` 会把 15 位之后的精度丢掉, 与逐笔
    /// `sum(pnl)` 对不上(实测差 1.75e-7)。
    final_equity: crate::money::Amount,
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

/// 扫描信号并模拟成交。
///
/// 时序不变量: 信号 bar `i` 只把目标方向**入队**, 直到 fill bar `i + delay`
/// 才改变 `cur_pos`/`cash`。因此 bar `i` 的权益只由 bar `i` 及之前已发生的
/// 成交决定, 不会按未来成交价盯市(见 `equity_curve_does_not_mark_to_future_fill_price`)。
///
/// `collect_curves`: 是否物化两条权益曲线与逐笔 [`Trade`]。false 时曲线由
/// [`CurveAccum`] 在线累计(O(1)); 但 [`TradeAccum`] 仍保留逐笔 pnl 序列
/// (O(交易笔数), 最坏 O(T))用于 SQN/偏度/峰度/分半 PF —— 不保留 T×16B 的两条曲线。
///
/// `open` 允许比 `close` 短(公共 API 的宽松契约): 缺失的开盘价由 [`fill_at`]
/// 退回收盘价, **不会**因此少回测末尾的 bar。旧实现把 `open.len()` 也算进
/// `n` 的 `min`, 于是 `open = []` 时整段信号被静默丢弃、一笔都不成交。
fn scan_trades(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    time: &[i64],
    initial_cash: crate::money::Amount,
    exec: &ExecConfig,
    collect_curves: bool,
) -> crate::error::Result<ScanOut> {
    let n = close.len().min(signals.len());
    let ts = |i: usize| if i < time.len() { time[i] } else { i as i64 };
    // 钱账本: 全部定点十进制(`domain-fintech`: 不用 f64 承载货币)。
    let mut cash = initial_cash;
    let mut pos_qty = crate::money::Amount::ZERO;
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
    let mut eq_acc = CurveAccum::new(initial_cash.to_f64());
    let mut fx_acc = CurveAccum::new(initial_cash.to_f64());
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
    // 同时保留**精确定点值**: 报告的 `final_equity` 不能从 f64 曲线回造。
    let mut tail_eq = initial_cash.to_f64();
    let mut tail_eq_amt = initial_cash;
    let mut tail_feq = initial_cash.to_f64();
    let mut fixed_realized = crate::money::Amount::ZERO;
    let mut fixed_qty = crate::money::Amount::ZERO;
    let mut fixed_entry = 0.0;
    // 破产告警只打一次, 避免逐 bar 刷屏。
    let mut bankrupt_warned = false;

    let mut trade_mfe: f64 = 0.0;
    let mut trade_mae: f64 = 0.0;

    // 信号在 fill bar 才生效: 每个 bar 算出的目标方向要延迟 `delay` 根 bar 再应用。
    // 队列只保存尚未到期的目标(O(delay) 内存), 不物化 O(T) 的目标序列。
    //
    // 旧实现把 `cur_pos`/`cash` 在**信号 bar** 就改掉、却用 `i + delay` 的价格成交,
    // 于是权益曲线在成交前就按未来价格盯市 —— Sharpe / 回撤 / Sortino 全部被污染。
    // 现在所有状态变更都推迟到 fill bar, 曲线在 bar i 只反映 bar i 及之前已发生的成交。
    let delay = exec.delay();
    let mut pending: std::collections::VecDeque<Side> =
        std::collections::VecDeque::with_capacity(delay.min(n));

    for i in 0..n {
        // 1) 先处理本 bar 到期的成交(信号 bar `i - delay` 的目标方向)。
        if i >= delay
            && let Some(due) = pending.pop_front()
        {
            // 反手 / 平仓: 先按本 bar 成交价了结旧仓。
            if cur_pos.is_open() && cur_pos != due {
                let exit_side = cur_pos.opposite();
                // 成交价非法(非有限/非正)时 `fill_at` 返回 None —— 此时**放弃
                // 本次平仓**(持仓延续到下一根), 而不是用会污染全部指标的
                // NaN/0 价格结算。
                if let Some(exit_price) = fill_at(i, exit_side, close, open, exec) {
                    let cargs = CloseArgs {
                        cur_pos,
                        pos_qty,
                        pos_entry,
                        pos_entry_time,
                        pos_entry_bar,
                        exit_price,
                        exit_time: ts(i),
                        exit_bar: i,
                        initial_cash,
                        trade_mfe,
                        trade_mae,
                        fee_rate: exec.fee_rate,
                        reason: ExitReason::Signal,
                    };
                    let out = close_outcome(cargs)?;
                    cash = cash.plus(out.cash_delta)?;
                    acc.push(out)?;
                    if collect_curves {
                        trades.push(materialize_trade(&cargs, out));
                    }
                    fixed_realized = fixed_realized.plus(out.fixed_pnl)?;
                    fixed_qty = crate::money::Amount::ZERO;
                    cur_pos = Side::Flat;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                }
            }

            // 开仓(含反手后的立刻反向开仓)。平仓失败时 `cur_pos` 仍持仓, 这里
            // 不会开新仓, 从而保证同一 bar 不会同时持有多空两条腿。
            //
            // **破产保护**: 现金 <= 0 时禁止开仓。成交量 = `cash / price`, 现金为负
            // 会得到负的数量 —— 空头"负负得正"变成做多、多头则越亏越多, 且现金
            // 永远无法回正。旧实现没有这道闸门, 爆仓后仍可无限开仓。
            if !cur_pos.is_open() && due.is_open() {
                if cash <= crate::money::Amount::ZERO {
                    if !bankrupt_warned {
                        tracing::warn!(
                            bar = i,
                            cash = %cash,
                            "现金 <= 0, 拒绝再开仓(账户已破产)"
                        );
                        bankrupt_warned = true;
                    }
                } else if let Some(entry_price) = fill_at(i, due, close, open, exec) {
                    let qty = cash.div_price(entry_price)?;
                    let fee = qty.mul_price(entry_price)?.mul_ratio(exec.fee_rate)?;
                    pos_entry = entry_price;
                    pos_entry_time = ts(i);
                    pos_entry_bar = i;
                    pos_qty = qty;
                    fixed_qty = initial_cash.div_price(entry_price)?;
                    fixed_entry = entry_price;
                    let notional = qty.mul_price(entry_price)?;
                    if due == Side::Long {
                        cash = cash.minus(notional.plus(fee)?)?;
                    } else {
                        cash = cash.plus(notional.minus(fee)?)?;
                    }
                    cur_pos = due;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                }
            }
        }

        // 2) 浮动盈亏只统计已成交持仓(fill bar 之后), fill 前的漂移不计入 MFE/MAE。
        if cur_pos.is_open() {
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

        // 3) 权益曲线是**序列**(供 `CurveAccum` 算对数收益、供 PBO 的 CSCV 用),
        // 需要超越函数 ⇒ 仍是 `f64`; 但它的**来源**是定点十进制账本。
        //
        // **价格非有限时按"已实现权益"计**(而不是让金额运算报错): 框架的既有
        // 契约是"非法价格 ⇒ 放弃成交 ⇒ 指标退化为 0 而非 NaN"(P7), 整轮回测
        // **不应**因一根 NaN bar 而失败。旧实现这里会产出 NaN 权益(被
        // `log_ret` 的定义域保护吃掉, 记 0 收益); 现在退化为"现金 + 浮动盈亏
        // 不可估 ⇒ 只记现金" —— 同样是有限的, 且不再把 NaN 写进曲线。
        let px = close[i];
        let eq_amt = match cur_pos {
            Side::Long if px.is_finite() => cash.plus(pos_qty.mul_price(px)?)?,
            Side::Short if px.is_finite() => cash.minus(pos_qty.mul_price(px)?)?,
            _ => cash,
        };
        let eq = eq_amt.to_f64();
        let feq = match cur_pos {
            Side::Long if px.is_finite() => initial_cash
                .plus(fixed_realized)?
                .plus(fixed_qty.mul_price(px - fixed_entry)?)?,
            Side::Short if px.is_finite() => initial_cash
                .plus(fixed_realized)?
                .plus(fixed_qty.mul_price(fixed_entry - px)?)?,
            _ => initial_cash.plus(fixed_realized)?,
        }
        .to_f64();
        if i + 1 < n {
            eq_acc.push(eq);
            fx_acc.push(feq);
            if collect_curves {
                equity_curve.push(eq);
                fixed_curve.push(feq);
            }
        } else {
            tail_eq = eq;
            tail_eq_amt = eq_amt;
            tail_feq = feq;
        }

        // 4) 本 bar 的信号只影响 `i + delay` 的成交, 先算出新目标并排队。
        target = next_target(target, signals[i]);
        pending.push_back(target);
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
            let out = close_outcome(cargs)?;
            cash = cash.plus(out.cash_delta)?;
            acc.push(out)?;
            if collect_curves {
                trades.push(materialize_trade(&cargs, out));
            }
            fixed_realized = fixed_realized.plus(out.fixed_pnl)?;
            // 强制平仓: 末根权益由盯市值改写为已实现 cash(扣掉平仓费与滑点)。
            tail_eq = cash.to_f64();
            tail_eq_amt = cash;
            tail_feq = initial_cash.plus(fixed_realized)?.to_f64();
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

    Ok(ScanOut {
        metrics: MetricsSource {
            acc,
            eq: eq_acc,
            fx: fx_acc,
            final_equity: tail_eq_amt,
        },
        detail: DetailArtifacts {
            trades,
            equity: equity_curve,
            fixed: fixed_curve,
        },
        collect_curves,
    })
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
/// 与「物化曲线后单遍遍历」**逐位一致**, 故 [`simulate`] 可省掉 O(T) 的曲线内存。
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
    fn push(&mut self, o: CloseOutcome) -> crate::error::Result<()> {
        self.num_trades += 1;
        self.pnls.push(o.pnl.to_f64());
        self.fixed_pnls.push(o.fixed_pnl.to_f64());
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
fn compute_metrics(
    out: &MetricsSource,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
) -> crate::error::Result<SummaryMetrics> {
    let n = out.eq.len();
    if n < 2 {
        return Ok(SummaryMetrics::default());
    }

    // 曲线统计全部来自在线累加器(与物化口径逐位一致, 见 CurveAccum 的文档)。
    let st = out.eq.stats(bars_per_year);
    // 末点权益取**账本精确值**, 而不是 f64 曲线的最后一点: 后者再经
    // `Amount::from_f64` 回造会丢掉 15 位之后的有效数字, 与逐笔 pnl 之和对不上。
    let final_eq = out.final_equity.to_f64();
    let total_return = if initial_cash > crate::money::Amount::ZERO {
        out.final_equity.minus(initial_cash)?.to_f64() / initial_cash.to_f64()
    } else {
        0.0
    };
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
        if years > 0.0 && initial_cash.to_f64() > 0.0 && final_eq > 0.0 {
            // CAGR 是**比率**(无量纲)⇒ f64; 金额相除时取各自的 f64 视图。
            (final_eq / initial_cash.to_f64()).powf(1.0 / years) - 1.0
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
    // 金额累计走 `Amount`(定点十进制); 下面的 `pnls` 序列是**统计量**口径(f64)。
    let total_win = out.acc.total_win;
    let total_loss = out.acc.total_loss;
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
        // 保本(p == 0)既不算赢也不算亏, 且**不打断**连胜/连亏段; runs 检验
        // 也直接跳过它(带并列值的符号序列没有唯一的 runs 定义, 忽略是标准做法)。
        if p > 0.0 {
            num_wins += 1;
            cw += 1;
            cl = 0;
            if cw > max_cw {
                max_cw = cw;
            }
        } else if p < 0.0 {
            num_losses += 1;
            cl += 1;
            cw = 0;
            if cl > max_cl {
                max_cl = cl;
            }
        }
        if p != 0.0 {
            let cur_win = p > 0.0;
            match prev_win {
                None => runs = 1,
                Some(prev) if prev != cur_win => runs += 1,
                _ => {}
            }
            prev_win = Some(cur_win);
        }

        let d = p - pnl_mean;
        let d2 = d * d;
        pnl_m2 += d2;
        pnl_m3 += d2 * d;
        pnl_m4 += d2 * d2;
    }

    let avg_win = if num_wins > 0 {
        total_win.div_count(i64::from(num_wins))?
    } else {
        crate::money::Amount::ZERO
    };
    let avg_loss = if num_losses > 0 {
        total_loss.div_count(i64::from(num_losses))?
    } else {
        crate::money::Amount::ZERO
    };
    // 下面的比值类指标是**无量纲比率** ⇒ f64。
    let profit_factor = if total_loss > crate::money::Amount::ZERO {
        total_win.to_f64() / total_loss.to_f64()
    } else {
        total_win.to_f64().max(0.0)
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

    let wl_ratio = if avg_loss > crate::money::Amount::ZERO {
        avg_win.to_f64() / avg_loss.to_f64()
    } else {
        0.0
    };

    let skewness = if nt > 2.0 && pnl_m2 > 0.0 {
        // 总体矩 m2 = Σd²/n、m3 = Σd³/n; g1 = m3 / m2^1.5 是总体偏度。
        // 再乘 Fisher-Pearson 小样本校正 √(n(n-1))/(n-2)。
        // 旧实现把**样本**标准差 √(m2/(n-1)) 当作 m2^0.5, 结果比标准校正偏度
        // 小 ((n-1)/n)^1.5(n=10 时约低 14%)。
        let m2 = pnl_m2 / nt;
        let m3 = pnl_m3 / nt;
        let g1 = m3 / m2.powf(1.5);
        g1 * (nt * (nt - 1.0)).sqrt() / (nt - 2.0)
    } else {
        0.0
    };

    let kurtosis = if nt > 3.0 && pnl_m2 > 0.0 {
        // 总体矩 m2/m4 + Fisher 小样本校正:
        //   ((n²-1) / ((n-2)(n-3))) · m4/m2² - 3(n-1)² / ((n-2)(n-3))
        // 旧实现在第一项多乘了一个 n, 峰度被系统性放大。
        let n = nt;
        let m2 = pnl_m2 / n;
        let m4 = pnl_m4 / n;
        ((n * n - 1.0) / ((n - 2.0) * (n - 3.0))) * (m4 / (m2 * m2))
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
        } else if f < 0.0 {
            fixed_loss += f.abs();
        }
    }
    let fixed_pf = if fixed_loss > 0.0 {
        fixed_win / fixed_loss
    } else {
        fixed_win.max(0.0)
    };
    let fixed_expectancy_bps = if nt > 0.0 {
        out.acc.total_fixed_pnl.to_f64() / nt / initial_cash.to_f64() * 10_000.0
    } else {
        0.0
    };
    let fx_st = out.fx.stats(bars_per_year);
    Ok(SummaryMetrics {
        total_return_pct: total_return * 100.0,
        total_return,
        sharpe_ratio: sharpe,
        sortino_ratio: sortino,
        max_drawdown_pct: max_dd * 100.0,
        // 胜率分母是**已决笔数**(胜 + 负), 保本不进分母: 把零盈亏算作
        // 一次非胜交易会系统性拉低胜率, 与"排除出亏损统计"的口径自相矛盾。
        win_rate: {
            let decided = num_wins + num_losses;
            if decided > 0 {
                num_wins as f64 / f64::from(decided) * 100.0
            } else {
                0.0
            }
        },
        profit_factor,
        calmar_ratio: calmar,
        num_trades,
        avg_win,
        avg_loss,
        expectancy,
        consecutive_wins: max_cw,
        consecutive_losses: max_cl,
        final_equity: out.final_equity,
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
    })
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
/// **不物化权益曲线**(曲线统计 O(1))。`--detail` 开关不影响任何指标数值,
/// 详见 `crate::lib` 的不变量 2。
///
/// 注意: 逐笔 pnl 序列仍需保留用于 SQN/偏度/峰度/分半 PF, 最坏 O(交易笔数),
/// 但远小于「每条 bar 两条 f64 曲线」的 O(T) 物化成本。
pub fn simulate(
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SummaryMetrics> {
    exec.validate_rates()?;
    let n = close.len().min(signals.len());
    if n < 2 {
        // 无成交 ⇒ 权益就是初始资金; 执行口径仍需回填, 否则报告里同一列
        // 会出现"数据不足的行是 0、正常行是真实值"的混排。
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(summary);
    }
    // collect_curves = false: 不物化曲线与逐笔 Trade; 曲线统计在线累计,
    // 逐笔 pnl 仍保留(O(交易笔数))用于高阶矩与分半 PF。
    let out = scan_trades(close, open, signals, &[], initial_cash, exec, false)?;
    debug_assert!(
        !out.collect_curves && out.detail.trades.is_empty(),
        "simulate 不得物化落盘产物"
    );
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);
    Ok(summary)
}

/// 执行一次回测并额外产出逐笔交易与两条曲线。
///
/// 指标经与 [`simulate`] **同一条**路径计算, 二者逐位一致。
pub fn simulate_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<SimResult> {
    exec.validate_rates()?;
    let n = close.len().min(signals.len());
    if n < 2 {
        let mut summary = SummaryMetrics {
            final_equity: initial_cash,
            ..SummaryMetrics::default()
        };
        summary.apply_exec(exec);
        return Ok(SimResult {
            summary,
            trades: Vec::new(),
            equity_curve: Vec::new(),
            fixed_curve: Vec::new(),
        });
    }
    // detailed 路径要落盘, 故额外物化逐笔 Trade 与两条曲线。
    // 注意指标仍只经 `out.metrics` 计算 —— 与 `simulate` 完全同源。
    let out = scan_trades(close, open, signals, time, initial_cash, exec, true)?;
    let mut summary = compute_metrics(&out.metrics, initial_cash, bars_per_year)?;
    summary.apply_exec(exec);

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

    Ok(SimResult {
        summary,
        trades: detail.trades,
        equity_curve,
        fixed_curve: detail.fixed,
    })
}

/// 批量回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch(
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SummaryMetrics>> {
    signal_batch
        .par_iter()
        .map(|signals| simulate(close, open, signals, initial_cash, bars_per_year, exec))
        .collect::<crate::error::Result<Vec<_>>>()
}

/// 批量详细回测(rayon 并行)。返回顺序与 `signal_batch` 一一对应。
pub fn simulate_batch_detailed(
    time: &[i64],
    close: &[f64],
    open: &[f64],
    signal_batch: &[Vec<Signal>],
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
    exec: &ExecConfig,
) -> crate::error::Result<Vec<SimResult>> {
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
        .collect::<crate::error::Result<Vec<_>>>()
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
        let m = simulate(
            &[],
            &[],
            &[],
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("空输入必须成功返回默认值");
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_no_signals_hold_value() {
        let close = vec![100.0; 100];
        let sig = vec![Signal::Hold; 100];
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert!((m.total_return_pct).abs() < 1e-10);
    }

    #[test]
    fn test_buy_and_hold() {
        let close: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        let mut sig = vec![Signal::Hold; 100];
        sig[0] = Signal::Buy;
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
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
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 1);
        assert_eq!(m.delay_bars, 1);
        // 进 open[1]=100.5, 出 close[3]=105; 名义 1000/100.5。
        let qty = 1000.0 / 100.5;
        let expect = (105.0 - 100.5) * qty;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
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

    /// **入口契约**: 负/非有限费率与滑点必须在 `ExecConfig` 构造期拒绝。
    ///
    /// 配置路径已有 newtype 保证, 但 `ExecConfig` 是公开引擎 API; 负费率会让
    /// 每笔交易"倒赚", `NaN` 会沿权益曲线污染全部指标。两者都不应等到回测
    /// 结束才以 `NaN` 的形式暴露。
    #[test]
    fn exec_rejects_negative_or_nonfinite_costs() {
        for (fee, slip) in [
            (-1.0, 0.0),
            (0.0, -1.0),
            (f64::NAN, 0.0),
            (0.0, f64::NAN),
            (f64::INFINITY, 0.0),
            (0.0, f64::INFINITY),
            (f64::NEG_INFINITY, 0.0),
        ] {
            assert!(
                ExecConfig::try_from_bps_fill(1, fee, slip, false).is_err(),
                "fee={fee}, slip={slip} 必须被拒"
            );
        }
        let ok = ExecConfig::try_from_bps_fill(1, 2.0, 1.0, true).expect("合法成本应通过");
        assert!((ok.fee_rate - 0.0002).abs() < 1e-15);
        assert!((ok.slip_rate - 0.0001).abs() < 1e-15);
        let panicked = std::panic::catch_unwind(|| ExecConfig::from_bps(1, -1.0, 0.0)).is_err();
        assert!(panicked, "from_bps 遇负费率应 panic(调用方 bug)");
    }

    /// 公开字段仍可被直接构造; 引擎入口必须再校验一次, 不能只依赖构造函数。
    #[test]
    fn engine_rejects_invalid_exec_fields_even_without_constructor() {
        let prices = [100.0, 101.0, 102.0];
        let bad = ExecConfig {
            fee_rate: -0.01,
            ..ExecConfig::default()
        };
        let err = simulate(
            &prices,
            &prices,
            &[],
            crate::money::Amount::from_i64(1000),
            252.0,
            &bad,
        )
        .expect_err("负费率必须在引擎入口被拒");
        assert!(format!("{err}").contains("fee_rate"), "{err}");
        let bad_nan = ExecConfig {
            slip_rate: f64::NAN,
            ..ExecConfig::default()
        };
        assert!(
            simulate(
                &prices,
                &prices,
                &[],
                crate::money::Amount::from_i64(1000),
                252.0,
                &bad_nan,
            )
            .is_err(),
            "NaN 滑点必须在引擎入口被拒"
        );
        let bad_huge = ExecConfig {
            fee_rate: f64::MAX,
            ..ExecConfig::default()
        };
        assert!(
            simulate(
                &prices,
                &prices,
                &[],
                crate::money::Amount::from_i64(1000),
                252.0,
                &bad_huge,
            )
            .is_err(),
            "换算回 bps 会溢出的费率必须在引擎入口被拒"
        );
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
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 2);
        // 多头: 进 101.0 出 100.5 → (100.5-101.0)/101.0*1000。
        let expect_long = (100.5 - 101.0) / 101.0 * 1000.0;
        // 空头: 进 100.5, 末根 107 收盘离场 → (100.5-107)/100.5*1000。
        let expect_short = (100.5 - 107.0) / 100.5 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - (expect_long + expect_short)).abs() < 1e-9,
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
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 2);
        // 多头: 进 102.0 出 105.0 → (105.0-102.0)/102.0*1000。
        let expect_long = (105.0 - 102.0) / 102.0 * 1000.0;
        // 空头: 进 105.0, 末根 107 收盘离场 → (105.0-107)/105.0*1000。
        let expect_short = (105.0 - 107.0) / 105.0 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - (expect_long + expect_short)).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
    }

    /// 权益曲线必须只反映**已成交**持仓: 信号 bar 不能用未来 fill bar 的价格盯市。
    ///
    /// 旧实现在 bar0 收到 Buy 就立刻把 `cur_pos`/`cash` 改成长仓, 却用
    /// `open[1] = 200` 作为成交价: `cash = 0`、`qty = 5`, 于是 bar0 权益
    /// `= qty × close[0] = 500` —— 在**尚未成交**的 bar 上凭空产生 50% 亏损,
    /// 整条曲线与 Sharpe/回撤全部失真。
    #[test]
    fn equity_curve_does_not_mark_to_future_fill_price() {
        let close = vec![100.0, 200.0, 200.0];
        let open = vec![100.0, 200.0, 200.0];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold];
        let d = simulate_detailed(
            &[0, 1, 2],
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        // bar0 尚未成交(初始资金); bar1 开盘成交后按 close[1] 盯市; bar2 收盘离场。
        for (i, (_, eq)) in d.equity_curve.iter().enumerate() {
            assert!(
                (eq - 1000.0).abs() < 1e-9,
                "bar{i} 权益 {eq} 应为 1000(成交前不得按未来价盯市)"
            );
        }
        // 零收益往返: 波动为 0, Sharpe 必须是 0 而不是被伪波动放大到 11.22。
        assert_eq!(d.summary.sharpe_ratio, 0.0);
        assert_eq!(d.summary.max_drawdown_pct, 0.0);
        assert_eq!(d.trades.len(), 1);
        assert!((d.trades[0].entry_price - 200.0).abs() < 1e-12);
    }

    /// `delay_bars = 2`: 信号 bar0 的目标方向要到 bar2 才成交; bar1 权益必须仍是
    /// 初始资金, 交易的实际成交 bar 也必须是 fill bar(而非信号 bar)。
    #[test]
    fn delay_two_defers_position_until_the_second_fill_bar() {
        let close = vec![100.0, 100.0, 100.0, 120.0, 120.0];
        let open = close.clone();
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let d = simulate_detailed(
            &[0, 1, 2, 3, 4],
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::from_bps(2, 0.0, 0.0),
        )
        .expect("合法回测必须成功");
        assert_eq!(d.equity_curve[0].1, 1000.0, "bar0 未成交");
        assert_eq!(d.equity_curve[1].1, 1000.0, "bar1 仍未成交");
        assert_eq!(d.equity_curve[3].1, 1200.0, "bar3 应按 close=120 盯市");
        assert_eq!(d.trades.len(), 2);
        // 多头: bar2 开 100 -> bar4 开 120; 持仓 2 bar。
        assert!((d.trades[0].entry_price - 100.0).abs() < 1e-12);
        assert!((d.trades[0].exit_price - 120.0).abs() < 1e-12);
        assert_eq!(d.trades[0].bars_held, 2);
        assert_eq!(d.trades[0].entry_time, 2);
        assert_eq!(d.trades[0].exit_time, 4);
    }

    /// 逐笔 `pnl` 必须包含**开仓**佣金: `sum(trade.pnl) == final_equity - initial`。
    ///
    /// 旧实现只扣平仓佣金, 逐笔 pnl 系统性偏乐观, win/loss/PF/SQN 全部被污染。
    #[test]
    fn trade_pnl_sums_to_equity_change_with_both_fees() {
        let close = vec![
            100.0, 104.0, 103.0, 109.0, 107.0, 111.0, 110.0, 114.0, 112.0,
        ];
        let open = vec![
            100.0, 101.0, 105.0, 102.0, 110.0, 106.0, 112.0, 109.0, 115.0,
        ];
        let sig = vec![
            Signal::Buy,
            Signal::Hold,
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Hold,
            Signal::Buy,
            Signal::Hold,
            Signal::Close,
        ];
        let initial = crate::money::Amount::from_i64(10_000);
        let exec = ExecConfig::from_bps(1, 10.0, 5.0);
        let d = simulate_detailed(&[], &close, &open, &sig, initial, 252.0, &exec)
            .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 3, "应覆盖多/空/末尾强平三条路径");
        let mut sum = crate::money::Amount::ZERO;
        for t in &d.trades {
            sum = sum.plus(t.pnl).expect("金额累加不得溢出");
        }
        let delta = d
            .summary
            .final_equity
            .minus(initial)
            .expect("金额相减不得溢出");
        let diff = sum.minus(delta).expect("金额相减不得溢出").to_f64().abs();
        assert!(
            diff < 1e-6,
            "sum(trade.pnl)={sum:?} 与 final-initial={delta:?} 不一致(diff={diff})"
        );
    }

    /// 偏度/峰度必须匹配标准小样本校正公式(pandas `skew()` / `kurt()` 口径)。
    ///
    /// 样本 `[1,2,3,4,10]`: 校正偏度 = 1.6970562748477143, 超额峰度 = 3.152。
    /// 旧实现偏度小 `((n-1)/n)^1.5`、峰度第一项多乘 `n`。
    #[test]
    fn skewness_and_kurtosis_match_corrected_sample_moments() {
        let acc = TradeAccum {
            num_trades: 5,
            pnls: vec![1.0, 2.0, 3.0, 4.0, 10.0],
            ..TradeAccum::default()
        };
        let mut eq = CurveAccum::new(1000.0);
        eq.push(1000.0);
        eq.push(1000.0);
        let mut fx = CurveAccum::new(1000.0);
        fx.push(1000.0);
        fx.push(1000.0);
        let m = compute_metrics(
            &MetricsSource {
                acc,
                eq,
                fx,
                final_equity: crate::money::Amount::from_i64(1000),
            },
            crate::money::Amount::from_i64(1000),
            252.0,
        )
        .expect("指标计算必须成功");
        assert!(
            (m.skewness - 1.697_056_274_847_714_3).abs() < 1e-12,
            "skewness={}",
            m.skewness
        );
        assert!(
            (m.kurtosis - 3.152).abs() < 1e-12,
            "kurtosis={}",
            m.kurtosis
        );
    }

    #[test]
    fn test_buy_hold_first_close_to_last_close() {
        // 100→110 名义 1000 → +100/+0.1; 不足 2 根或首价非法 → 0。
        let (pnl, ret) = buy_hold_pnl(&[100.0, 105.0, 110.0], crate::money::Amount::from_i64(1000))
            .expect("合法基准必须成功");
        assert!((pnl.to_f64() - 100.0).abs() < 1e-9, "{pnl}");
        assert!((ret - 0.1).abs() < 1e-12, "{ret}");
        assert_eq!(
            buy_hold_pnl(&[100.0], crate::money::Amount::from_i64(1000))
                .expect("数据不足应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
        assert_eq!(
            buy_hold_pnl(&[], crate::money::Amount::from_i64(1000))
                .expect("非法输入应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
        assert_eq!(
            buy_hold_pnl(&[0.0, 110.0], crate::money::Amount::from_i64(1000))
                .expect("非法输入应返回零, 不报错"),
            (crate::money::Amount::ZERO, 0.0)
        );
    }

    #[test]
    fn test_fee_and_slippage_deducted_per_side() {
        // fee 10bp + slip 10bp 双边: 进出各扣一次。用 Close 离场以隔离单笔数学。
        // delay 恒 >= 1: bar0 信号在 bar1 成交, bar1 的 Close 在 bar2 成交。
        let close = vec![100.0, 100.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Close, Signal::Hold];
        let exec = ExecConfig::from_bps(1, 10.0, 10.0);
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 1);
        // 进 open[1]*1.001 = 100.1, 出 open[2]*0.999 = 109.89; 名义 1000/100.1;
        // 毛利 (109.89-100.1)*qty - 进出佣金 (100.1+109.89)*qty*0.001。
        let qty = 1000.0 / 100.1;
        let expect = (109.89 - 100.1) * qty - (100.1 + 109.89) * qty * 0.001;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
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
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(m.num_trades, 0);
    }

    #[test]
    fn test_fixed_leg_matches_hand_calc() {
        // fixed 腿: 单多 100→110, 名义 1000 → +100, 期望 +1000bp。
        // delay 恒 >= 1: bar0 进(填在 bar1), bar2 的 Close 填在 bar3。
        let close = vec![100.0, 100.0, 110.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Close, Signal::Hold];
        let m = simulate(
            &close,
            &open,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
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
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
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
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
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
        let results = simulate_batch(
            &close,
            &close,
            &batch,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法批量回测必须成功");
        let r1 = simulate(
            &close,
            &close,
            &sig1,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        let r2 = simulate(
            &close,
            &close,
            &sig2,
            crate::money::Amount::from_i64(10_000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");

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

    /// [`simulate`] 必须**不物化**落盘产物(曲线/Trade 均不保留); 这是 O2 的全部意义。
    ///
    /// 允许保留 O(交易笔数) 的 pnl 统计序列 —— 它远小于 O(T) 的双曲线,
    /// 且是 SQN/偏度/峰度/分半 PF 的必需输入。
    #[test]
    fn test_simulate_does_not_materialize_equity_curve() {
        let close = make_prices(100_000);
        let mut sig = vec![Signal::Hold; 100_000];
        sig[0] = Signal::Buy;
        let exec = ExecConfig::default();
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            365.0,
            &exec,
        )
        .expect("合法回测必须成功");
        let d = simulate_detailed(
            &[],
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(10_000),
            365.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(d.equity_curve.len(), 100_000, "detailed 必须物化曲线");
        assert_eq!(d.fixed_curve.len(), 100_000);

        // Q2 结构性保证: `scan_trades` 在 collect_curves=false 时不得产出任何
        // DetailArtifacts(逐笔 Trade + 两条曲线)。类型上由 Option 表达, 此处
        // 在 debug 构建下由 debug_assert! 复核。
        let out = scan_trades(
            &close,
            &close,
            &sig,
            &[],
            crate::money::Amount::from_i64(10_000),
            &exec,
            false,
        )
        .expect("合法回测必须成功");
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
        let m = simulate(
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            8760.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
        assert!(
            m.final_equity.to_f64() < 0.0,
            "该用例应确实穿零: {}",
            m.final_equity
        );
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
            simulate(
                &open,
                &open,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &ExecConfig::default(),
            )
            .expect("合法回测必须成功")
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
            let a = simulate(
                &close,
                &close,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &exec,
            )
            .expect("合法回测必须成功");
            let b = simulate_detailed(
                &[],
                &close,
                &close,
                &sig,
                crate::money::Amount::from_i64(10_000),
                252.0,
                &exec,
            )
            .expect("合法回测必须成功");
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
                ("total_fixed_pnl", a.total_fixed_pnl.to_f64()),
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
                    "total_fixed_pnl" => b.summary.total_fixed_pnl.to_f64(),
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

    /// **回归 #2**: 现金 <= 0 后必须拒绝再开仓。
    ///
    /// 场景: 100 做空 → 300 平仓, 亏损 2 倍本金, 现金 = -1000; 随后再次做空。
    /// 旧实现直接按 `cash / price` 计算数量, 得到**负数量**(方向反转),
    /// 报告里出现无法解释的仓位与权益。
    #[test]
    fn no_reentry_after_cash_is_wiped_out() {
        let close = vec![100.0, 100.0, 300.0, 300.0, 300.0, 300.0];
        let sig = vec![
            Signal::Sell,
            Signal::Hold,
            Signal::Close,
            Signal::Sell,
            Signal::Hold,
            Signal::Hold,
        ];
        let exec = ExecConfig::from_bps_fill(1, 0.0, 0.0, true);
        let d = simulate_detailed(
            &[0, 1, 2, 3, 4, 5],
            &close,
            &close,
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &exec,
        )
        .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 1, "爆仓后不得再开仓");
        assert_eq!(d.summary.num_trades, 1);
        assert_eq!(
            d.summary.final_equity,
            crate::money::Amount::from_i64(-1000),
            "现金 = 1000 + 1000 - 3000 = -1000, 且不再产生新仓位"
        );
        for t in &d.trades {
            assert!(
                t.qty > crate::money::Amount::ZERO,
                "持仓数量必须为正(方向由 side 表达): {}",
                t.qty
            );
        }
    }

    /// **回归 #3**: `open` 缺失时成交价退回收盘价, 而不是把整段数据截掉。
    ///
    /// 旧实现 `n = min(close, signals, open)`, `open = []` 时 n = 0,
    /// `fill_at` 里精心写的回退分支**永远不可达**, 报告显示"零交易"。
    #[test]
    fn missing_open_falls_back_to_close_instead_of_truncating_bars() {
        let close = vec![100.0, 110.0, 120.0, 130.0];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let m = simulate(
            &close,
            &[],
            &sig,
            crate::money::Amount::from_i64(1000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("open 缺失是允许的宽松契约");
        assert_eq!(m.num_trades, 1, "不得因 open 缺失而少回测 bar");
        // bar0 信号 -> bar1 收盘(回退)成交, 末根 130 平仓。
        let expect = (130.0 - 110.0) / 110.0 * 1000.0;
        assert!(
            (m.total_fixed_pnl.to_f64() - expect).abs() < 1e-9,
            "{} vs {expect}",
            m.total_fixed_pnl
        );
    }

    /// **回归 #7**: `final_equity` 必须来自定点账本, 不得经 f64 曲线回造。
    ///
    /// 旧实现把末点权益先转 `f64` 再 `Amount::from_f64`, 只有约 15 位有效数字;
    /// 与逐笔 `sum(pnl)` 对不上(实测差 1.75e-7)。本用例用 13bp 佣金放大误差。
    #[test]
    fn final_equity_keeps_the_exact_decimal_ledger_value() {
        let price = 100.123_456_789;
        let close = vec![price, price];
        let sig = vec![Signal::Buy, Signal::Hold];
        let initial = crate::money::Amount::from_f64(1_000_000_000.0).expect("有限金额");
        let exec = ExecConfig::from_bps_fill(1, 13.0, 0.0, true);
        let d = simulate_detailed(&[0, 1], &close, &close, &sig, initial, 252.0, &exec)
            .expect("合法回测必须成功");
        assert_eq!(d.trades.len(), 1);
        // 开仓与平仓同价 ⇒ 期末权益 = 初始资金 - 两笔佣金, 全部为精确十进制。
        let exact_sum = initial.plus(d.trades[0].pnl).expect("金额累加不得溢出");
        assert_eq!(
            d.summary.final_equity, exact_sum,
            "final_equity 必须与逐笔 pnl 之和精确相等, 不能有 f64 往返误差"
        );
        let qty = initial.div_price(price).expect("正价格");
        let fee = qty
            .mul_price(price)
            .expect("有限价格")
            .mul_ratio(0.0013)
            .expect("有限费率");
        assert_eq!(
            d.summary.final_equity,
            initial.minus(fee).expect("金额").minus(fee).expect("金额")
        );
    }

    /// **回归 #10**: 零盈亏交易是"保本", 不得计入亏损/连亏/胜率分母。
    ///
    /// 序列 `[+1, 0, +2, -1, 0, -2, -3]`: 2 胜 / 3 负 / 2 保本。
    /// 旧实现把两个 0 算作亏损 ⇒ 胜率 2/7、连亏 4、avg_loss 被稀释到 1.2。
    #[test]
    fn breakeven_trades_are_not_losses() {
        let acc = TradeAccum {
            num_trades: 7,
            pnls: vec![1.0, 0.0, 2.0, -1.0, 0.0, -2.0, -3.0],
            total_win: crate::money::Amount::from_i64(3),
            total_loss: crate::money::Amount::from_i64(6),
            ..TradeAccum::default()
        };
        let mut eq = CurveAccum::new(1000.0);
        eq.push(1000.0);
        eq.push(1000.0);
        let mut fx = CurveAccum::new(1000.0);
        fx.push(1000.0);
        fx.push(1000.0);
        let m = compute_metrics(
            &MetricsSource {
                acc,
                eq,
                fx,
                final_equity: crate::money::Amount::from_i64(1000),
            },
            crate::money::Amount::from_i64(1000),
            252.0,
        )
        .expect("指标计算必须成功");
        assert!((m.win_rate - 40.0).abs() < 1e-12, "win_rate={}", m.win_rate);
        assert_eq!(m.consecutive_wins, 2, "保本不得打断连胜");
        assert_eq!(m.consecutive_losses, 3, "保本不得计入/打断连亏");
        assert!(
            (m.avg_loss.to_f64() - 2.0).abs() < 1e-12,
            "avg_loss 只能用 3 笔亏损作分母: {}",
            m.avg_loss
        );
        assert!(
            (m.profit_factor - 0.5).abs() < 1e-12,
            "profit_factor={}",
            m.profit_factor
        );
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
            crate::money::Amount::from_i64(10_000),
            252.0,
            &ExecConfig::default(),
        )
        .expect("合法回测必须成功");
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
