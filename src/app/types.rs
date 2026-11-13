//! 报告行与运行状态的公开类型。

use crate::engine;

#[derive(Debug, Clone)]
/// 单个参数求值的结果行, 即报告 `result.parquet` 的一行。
pub struct RunResult {
    /// 品种。
    pub symbol: String,
    /// 周期(如 `1d`)。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文; 四元组行主键 `(symbol, timeframe, strategy, params)` 之一。
    pub params: String,
    /// 全部回测指标。
    pub metrics: engine::SummaryMetrics,
    /// **原样保留**写入时的数值, 合并时不重算(见 `strategy_params_value` 的文档)。
    /// 0 表示"未记录"(旧缓存文件无此列)。
    pub strategy_params: u64,
    /// 同上, 人类可读的 JSON 编码原样保留。空串表示未记录。
    pub strategy_params_json: String,
    /// 执行口径: 信号到成交的 bar 间隔。
    pub delay_bars: u32,
    /// 执行口径: 单边佣金(bp)。
    pub fee_bps: f64,
    /// 执行口径: 单边逆向滑点(bp)。
    pub slippage_bps: f64,
    /// delay>0 时 fill bar 取收盘(true)还是开盘(false, 默认)。
    pub fill_close: bool,
    /// 被动基准(固定名义): 首根 close 买入、末根 close 卖出、无成本的 pnl
    /// (**金额**, 定点十进制)。
    pub buy_hold_pnl: crate::money::Amount,
    /// 被动基准总收益(=pnl/名义)。
    pub buy_hold_return: f64,
    /// 本次求值实际喂给策略的 bar 数。
    ///
    /// 为什么必须有: `params` 字符串只描述**参数**, 不描述**数据规模**。
    /// 同一 `period=400` 在 300 bar 与 20000 bar 上参数串完全相同, 但前者
    /// 策略窗口都不够、全程 Hold、指标全 0。没有这一列, 下游无法判断某个
    /// "全 0 结果"是真的还是数据不足导致的退化。
    pub bars_used: u32,
    /// 求值状态。见 [`RunStatus`]。
    pub status: RunStatus,
}

/// 单个参数求值的**可用性状态**。写入报告的 `status` 列。
///
/// 存在的理由: 退化结果的指标全是 0, 与"零波动低风险"的真实结论在数值上
/// **不可区分**。实测反例(同一 `params` 字符串 `period=400`):
///
/// - 1d / 300bar: `trades=0 / pnl=0 / sharpe=0`(策略窗口都不够, 全程 Hold)
/// - 1m / 20000bar: `trades=1310 / pnl=-1.0 / sharpe=-18.04`
///
/// 只靠指标列无法判断前者是"数据不够, 策略根本没跑起来"还是"跑了但确实无信号"。
/// 排序/取 top-N 时前者会挤掉真实结果且无人察觉。
///
/// 口径说明: `NoSignals` 同时覆盖"策略窗口 > 数据长度"与"该参数本就无信号"
/// 两种成因 —— 两者在引擎层不可区分(都是全 Hold)。**`bars_used` 列提供了区分
/// 所需的信息**, 两者必须配对解读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunStatus {
    /// 正常: 产生了成交。
    #[default]
    Ok,
    /// 全程 Hold: 没有任何非 Hold 信号, 全部指标为 0(退化结果)。
    NoSignals,
    /// 有非 Hold 信号但一笔未成交(如信号全落在末根, `delay >= 1` 时无未来 fill bar)。
    NoTrades,
    /// **该参数档位不可计算**(数据/参数不足, 例如 `period` 超过可用 bar 数)。
    ///
    /// 与 [`Self::NoSignals`] 的区别是本项目的核心不变量:
    /// - `NoSignals` = 这一档**跑了**, 但全程 Hold(可能本就无信号);
    /// - `Invalid` = 这一档**根本没算出来**(指标库返回错误), 被显式跳过。
    ///
    /// 引入本状态的原因: 在此之前, 66/67 个策略把"算不出来"**静默**降级为
    /// 全 Hold, 于是它在报告里长得和"零波动低风险"的真实结论**一模一样**
    /// (`trades=0 / pnl=0 / sharpe=0`) —— 对参数网格过拟合研究(本项目的 PBO
    /// 模块)而言, 这正是最该被发现的失败模式, 却无人察觉。
    ///
    /// 读表时**必须**把 `Invalid` 单独筛掉: 它不是"策略没信号", 而是"这一档
    /// 不可用", 把它混进统计会稀释样本并高估/低估胜率。
    Invalid,
    /// **账户已破产**: 权益(现金)降到 ≤ 0, 引擎拒绝再开仓。
    ///
    /// 与 [`Self::Invalid`] 不同: 这一档**跑出来了**, 只是资金曲线归零/转负。
    /// 典型成因是空头在上涨行情中亏损超过本金。旧引擎没有开仓闸门, 爆仓后
    /// 会继续以负现金计算持仓量(数量变负 ⇒ 方向反转), 报告里出现无法解释的
    /// 巨额权益; 现在只允许平掉已有仓位, 不再开新仓。
    ///
    /// 读表时同样**不能**把它当作正常 `ok` 参与排名/PBO: 收益率已失去意义
    /// (本金归零后任何比例都不可比)。
    Bankrupt,
}

impl RunStatus {
    /// 落盘用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NoSignals => "no_signals",
            Self::NoTrades => "no_trades",
            Self::Invalid => "invalid",
            Self::Bankrupt => "bankrupt",
        }
    }

    /// 从 `as_str` 的落盘文本解析。
    ///
    /// 用 `Option` 而不是 `Default`: 读到未知状态必须让调用方报错, 不能把
    /// "别的版本写的新状态"静默当成 `ok` 混入 PBO。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Self::Ok),
            "no_signals" => Some(Self::NoSignals),
            "no_trades" => Some(Self::NoTrades),
            "invalid" => Some(Self::Invalid),
            "bankrupt" => Some(Self::Bankrupt),
            _ => None,
        }
    }

    /// 从信号序列与回测指标判定状态。
    ///
    /// `Bankrupt` 优先于 `Ok`: 账户权益 ≤ 0 的运行即使产生过成交, 也不能作为
    /// 正常结果参与排名(收益率的分母已失效)。判定条件用**末点权益**而非
    /// 单笔盈亏 —— 爆仓可能由多笔累计亏损造成。
    #[must_use]
    pub fn classify(
        signals: &[crate::strategies::Signal],
        metrics: &engine::SummaryMetrics,
    ) -> Self {
        if metrics.num_trades > 0 && metrics.final_equity <= crate::money::Amount::ZERO {
            return Self::Bankrupt;
        }
        if metrics.num_trades > 0 {
            return Self::Ok;
        }
        if signals
            .iter()
            .any(|s| *s != crate::strategies::Signal::Hold)
        {
            Self::NoTrades
        } else {
            Self::NoSignals
        }
    }
}

#[derive(Debug, Clone)]
/// `--detail` 模式的落盘产物(逐笔交易 + 两条曲线)。
pub struct DetailResult {
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,
    /// 该档位的运行状态。
    ///
    /// `no_signals` / `no_trades` / `bankrupt` 也会产生曲线文件, 但**不得**参与
    /// PBO 排名: 它们的收益率没有可比性(全 0 或本金已归零)。状态随曲线落盘,
    /// `--pbo` 才能把它们显式筛掉而不是当成正常策略。
    pub status: RunStatus,
    /// 逐笔已平仓交易。
    pub trades: Vec<engine::Trade>,
    /// 复利腿逐 bar `(时间戳, 权益)`。
    pub equity_curve: Vec<(f64, f64)>,
    /// fixed 腿逐 bar 权益(与 bar 对齐), 写入 `{...}_fixedcurve.parquet` 供 `--pbo`。
    pub fixed_curve: Vec<f64>,
}
