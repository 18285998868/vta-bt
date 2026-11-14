//! 引擎的公开数据类型: 指标、交易与求值结果。

use super::exec::{ExecConfig, Side};

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
    /// 因固定百分比/ATR 止损离场的笔数。
    pub stop_loss_exits: u32,
    /// 因百分比/ATR 追踪止损或保本止损离场的笔数。
    pub trailing_stop_exits: u32,
    /// 历史字段: 策略止损线能力已移除, 当前版本恒为 0。
    ///
    /// 保留该字段与报告列只为兼容既有报告 schema 与旧 detail 文件。
    pub strategy_stop_exits: u32,
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

/// ATR 止损规则。
///
/// `mult` 是 ATR 倍数; `trailing=false` 为入场时固定的 ATR 止损,
/// `trailing=true` 为跟随有利极值移动的 ATR 追踪止损。ATR 使用 Wilder
/// 平滑, 入场时取**上一根已收盘 bar** 的 ATR(收盘成交取入场 bar 的 ATR),
/// 避免用入场后才走完的行情计算初始止损。

#[derive(Debug, Clone, PartialEq)]
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
    /// 固定百分比/ATR 止损触发。
    StopLoss,
    /// 百分比/ATR 追踪止损或保本止损触发。
    TrailingStop,
    /// 历史变体: 策略止损线能力已移除, 不再产生。
    ///
    /// 保留该变体只为兼容旧 detail 文件中的 `strategy_stop` 文本。
    StrategyStop,
    /// 数据末尾强制平仓。
    EndOfData,
}

impl ExitReason {
    /// 落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::StopLoss => "stop_loss",
            Self::TrailingStop => "trailing_stop",
            Self::StrategyStop => "strategy_stop",
            Self::EndOfData => "end_of_data",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// `--detail` 路径的一次求值结果。
pub struct SimResult {
    /// 全部指标(与 [`simulate`](super::simulate) 逐位一致)。
    pub summary: SummaryMetrics,
    /// 逐笔已平仓交易。
    pub trades: Vec<Trade>,
    /// 复利腿逐 bar `(时间戳, 权益)`。
    pub equity_curve: Vec<(f64, f64)>,
    /// fixed 腿逐 bar 权益(与 bar 对齐, `--pbo` 的 T×N 输入)。
    pub fixed_curve: Vec<f64>,
}
