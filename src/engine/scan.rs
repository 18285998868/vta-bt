//! 信号扫描: 目标方向状态机、成交与平仓结算。

use crate::strategies::{Signal, StopLines};

use super::accum::{CurveAccum, TradeAccum};
use super::exec::{ExecConfig, Side};
use super::sizing::{SizingConfig, entry_qty};
use super::stops::{
    AtrStopConfig, StopConfig, atr_at_entry, stop_exit_price, tighter_stop, wilder_atr,
};
use super::types::{ExitReason, Trade};

/// 信号 → **目标持仓方向** 的单步状态机。
///
/// 为什么不是一个数组: 原实现先物化一份 O(T) 的 `Vec<Side>`, 而 `scan_trades`
/// 只在**当前 bar** 用到 `positions[i]`。物化版本的代价是每次参数求值多分配
/// 一条与 bar 数等长的向量(T=1M 时 1 MB/档), 参数网格动辄 10 万档 ⇒ 百 GB 级
/// 的额外分配流量, 与本仓库"内存是第一约束"的设计前提直接冲突。
///
/// 状态机是**前缀累积**的, 故逐 bar 增量维护与先物化再索引**逐位等价**。
#[inline]
pub(super) fn next_target(prev: Side, sig: Signal) -> Side {
    match sig {
        Signal::Buy => Side::Long,
        Signal::Sell => Side::Short,
        Signal::Close => Side::Flat,
        Signal::ExitLong if prev == Side::Long => Side::Flat,
        Signal::ExitShort if prev == Side::Short => Side::Flat,
        Signal::ExitLong | Signal::ExitShort | Signal::Hold => prev,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CloseArgs {
    pub(super) cur_pos: Side,
    /// 持仓数量(金额量纲, 定点十进制)。
    pub(super) pos_qty: crate::money::Amount,
    pub(super) pos_entry: f64,
    pub(super) pos_entry_time: i64,
    pub(super) pos_entry_bar: usize,
    pub(super) exit_price: f64,
    pub(super) exit_time: i64,
    pub(super) exit_bar: usize,
    /// 本笔 fixed 腿的持仓数量(定点十进制)。开仓时按仓位模式确定,
    /// 平仓时必须复用同一数量, 不能按 initial_cash 重算。
    pub(super) fixed_qty: crate::money::Amount,
    pub(super) trade_mfe: f64,
    pub(super) trade_mae: f64,
    pub(super) fee_rate: f64,
    /// 平仓原因(信号反手 / 数据末尾强制)。
    pub(super) reason: ExitReason,
}

/// 方向 side 的成交价: 买入抬价、卖出压价(逆向滑点)。
pub(super) fn fill_price(side: Side, base: f64, slip_rate: f64) -> f64 {
    if side == Side::Long {
        base * (1.0 + slip_rate)
    } else {
        base * (1.0 - slip_rate)
    }
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
pub(super) fn valid_fill_price(p: f64) -> bool {
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
pub(super) fn fill_at(
    bar: usize,
    side: Side,
    close: &[f64],
    open: &[f64],
    exec: &ExecConfig,
) -> Option<f64> {
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
pub(super) struct CloseOutcome {
    /// 现金变动(定点十进制)。
    pub(super) cash_delta: crate::money::Amount,
    /// 本笔盈亏(定点十进制)。
    pub(super) pnl: crate::money::Amount,
    /// 本笔收益率(**比率**, 无量纲 ⇒ 仍是 `f64`)。
    pub(super) pnl_pct: f64,
    /// fixed 腿本笔盈亏(定点十进制)。
    pub(super) fixed_pnl: crate::money::Amount,
    pub(super) bars_held: i64,
    pub(super) max_runup: f64,
    pub(super) max_drawdown: f64,
    /// 平仓原因, 用于累计止损笔数。
    pub(super) reason: ExitReason,
}

/// 计算一次平仓的**钱**与收益率。
///
/// 钱的部分全部走 [`Amount`](crate::money::Amount)(定点十进制): 佣金、
/// 现金变动、单笔盈亏、fixed 腿盈亏。价格与费率仍是 `f64`(价格是连续量,
/// 费率无量纲), 但**钱**本身不再用 `f64` 承载。
///
/// # Errors
/// 金额运算溢出、或除以非法价格时返回 `AppError`(crate::error::AppError)。
pub(super) fn close_outcome(args: CloseArgs) -> crate::error::Result<CloseOutcome> {
    let CloseArgs {
        cur_pos,
        pos_qty,
        pos_entry,
        pos_entry_bar,
        exit_price,
        exit_bar,
        fixed_qty,
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
        reason: args.reason,
    })
}

/// 由标量结果物化完整 [`Trade`]。**仅 detailed 路径调用**。
pub(super) fn materialize_trade(args: &CloseArgs, out: CloseOutcome) -> Trade {
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

/// 平仓时需要更新的账本/产物引用。
///
/// 单独成结构体而不是把 6 个 `&mut` 参数平铺: 调用点只构造一次, 且
/// 「信号平仓 / 止损平仓 / 末根强平」三条路径共用同一段资金结算逻辑。
pub(super) struct CloseState<'a> {
    cash: &'a mut crate::money::Amount,
    acc: &'a mut TradeAccum,
    trades: &'a mut Vec<Trade>,
    fixed_realized: &'a mut crate::money::Amount,
    fixed_qty: &'a mut crate::money::Amount,
    collect_curves: bool,
}

/// 结算一笔平仓并更新所有账本/产物。
pub(super) fn close_at(args: CloseArgs, st: &mut CloseState<'_>) -> crate::error::Result<()> {
    let out = close_outcome(args)?;
    *st.cash = st.cash.plus(out.cash_delta)?;
    st.acc.push(out)?;
    if st.collect_curves {
        st.trades.push(materialize_trade(&args, out));
    }
    *st.fixed_realized = st.fixed_realized.plus(out.fixed_pnl)?;
    *st.fixed_qty = crate::money::Amount::ZERO;
    Ok(())
}

/// 指标计算的**唯一**数据源。
///
/// 两条路径(详细 / 非详细)**都**填充它, 且填充方式逐行相同 —— 这是
/// "报告数字不随 `--detail` 开关变化"的结构性保证: [`compute_metrics`](super::metrics::compute_metrics) 的签名
/// 收 `&MetricsSource`, 因此它在**类型上就看不到** [`DetailArtifacts`]。
///
/// 拆分自原先的单一 `ScanOut`: 那里 6 个字段并列, "指标只读其中 3 个"仅仅是
/// 约定(谁新增一个字段并顺手读它, 就会让两条路径漂移), 而非编译期约束。
#[derive(Debug)]
pub(super) struct MetricsSource {
    pub(super) acc: TradeAccum,
    pub(super) eq: CurveAccum,
    pub(super) fx: CurveAccum,
    /// 末根权益的**精确定点值**(不是从 f64 曲线回造的近似值)。
    ///
    /// 报告里的 `final_equity` 与 `total_return` 必须由账本直接给出: 先转成
    /// `f64` 再 `Amount::from_f64` 会把 15 位之后的精度丢掉, 与逐笔
    /// `sum(pnl)` 对不上(实测差 1.75e-7)。
    pub(super) final_equity: crate::money::Amount,
}

/// 仅详细路径需要落盘的产物。
///
/// 为空表示 `collect_curves == false`([`simulate`](super::simulate)); [`simulate_detailed`](super::simulate_detailed) 传
/// true 时填充。这些字段**不参与**任何指标计算 —— 指标只读 [`MetricsSource`]。
#[derive(Debug, Default)]
pub(super) struct DetailArtifacts {
    pub(super) trades: Vec<Trade>,
    pub(super) equity: Vec<f64>,
    pub(super) fixed: Vec<f64>,
}

/// `scan_trades` 的产出: 指标源 + (可选的)落盘产物。
#[derive(Debug)]
pub(super) struct ScanOut {
    /// 两条路径都有, 且填充方式完全一致。
    pub(super) metrics: MetricsSource,
    /// 仅 `collect_curves == true` 时有内容; 否则三个字段全空。
    pub(super) detail: DetailArtifacts,
    /// 是否物化了落盘产物。**不**用 `Option` 表达, 免得调用方为了取产物
    /// 而不得不写一个 `expect()`(那正是 `err-no-unwrap-prod` 反对的东西)。
    pub(super) collect_curves: bool,
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
///
/// `high` / `low` 只在配置了 [`StopConfig`] 或策略提供了 [`StopLines`] 时
/// 参与盘中止损判定; 两者都为空时完全不影响结果, 与旧路径逐位一致。
#[allow(clippy::too_many_arguments)]
pub(super) fn scan_trades(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    strategy_stops: Option<&StopLines>,
    time: &[i64],
    initial_cash: crate::money::Amount,
    exec: &ExecConfig,
    risk: &StopConfig,
    sizing: &SizingConfig,
    collect_curves: bool,
) -> crate::error::Result<ScanOut> {
    risk.validate()?;
    sizing.validate()?;
    let n = close.len().min(signals.len());
    if let Some(lines) = strategy_stops {
        lines.validate_len(n)?;
    }
    let atr_values = risk.atr.map(|cfg| wilder_atr(high, low, close, cfg.period));
    let sizing_atr = match *sizing {
        SizingConfig::Atr { period, .. } => Some(wilder_atr(high, low, close, period)),
        _ => None,
    };
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
        pnl_pcts: Vec::with_capacity(1024),
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

    // 持仓期止损状态。全部 `Option`: 未配置 / 已平仓时为 None, 不用 NaN 哨兵。
    // `pos_entry_at_close`: 本笔入场是否发生在 bar 收盘。收盘成交时当根
    // high/low 已在入场前走完, 故当根**不得**回溯触发止损。
    let mut pos_entry_at_close = false;
    let mut pos_stop_loss: Option<f64> = None;
    let mut pos_trail_extreme: Option<f64> = None;
    let mut pos_trail_stop: Option<f64> = None;
    let mut pos_strategy_stop: Option<f64> = None;
    let mut pos_breakeven_stop: Option<f64> = None;

    // 信号在 fill bar 才生效: 每个 bar 算出的目标方向要延迟 `delay` 根 bar 再应用。
    // 队列只保存尚未到期的目标(O(delay) 内存), 不物化 O(T) 的目标序列。
    // 声明在止损宏之前: 止损平仓时宏要把“stop 之前排队”的陈旧目标作废。
    let delay = exec.delay();
    let mut pending: std::collections::VecDeque<Side> =
        std::collections::VecDeque::with_capacity(delay.min(n));

    // 止损平仓与信号平仓共用同一套账本结算; 宏只负责把当前 bar 的止损
    // 状态、成交价与原因拼成 `CloseArgs`。用宏而不是闭包: 闭包会长期
    // 可变借用 `cash`/`acc`/`trades`, 与循环内其他读写冲突。
    macro_rules! try_stop_exit {
        ($bar:expr) => {{
            if cur_pos.is_open() {
                // 策略止损线是逐 bar 输出: 每根 bar 开始前取当前方向的价格线。
                // 无 line / NaN / 非正价格 => 该 bar 不设策略止损。
                pos_strategy_stop = match cur_pos {
                    Side::Long => strategy_stops.and_then(|s| s.long_at($bar)),
                    Side::Short => strategy_stops.and_then(|s| s.short_at($bar)),
                    Side::Flat => None,
                };
            }
            if cur_pos.is_open()
                && !($bar == pos_entry_bar && pos_entry_at_close)
                && let Some((base, reason)) = stop_exit_price(
                    $bar,
                    cur_pos,
                    high,
                    low,
                    open,
                    close,
                    pos_stop_loss,
                    pos_strategy_stop,
                    pos_trail_stop,
                    pos_breakeven_stop,
                )
            {
                let exit_price = fill_price(cur_pos.opposite(), base, exec.slip_rate);
                if valid_fill_price(exit_price) {
                    let cargs = CloseArgs {
                        cur_pos,
                        pos_qty,
                        pos_entry,
                        pos_entry_time,
                        pos_entry_bar,
                        exit_price,
                        exit_time: ts($bar),
                        exit_bar: $bar,
                        fixed_qty,
                        trade_mfe,
                        trade_mae,
                        fee_rate: exec.fee_rate,
                        reason,
                    };
                    close_at(
                        cargs,
                        &mut CloseState {
                            cash: &mut cash,
                            acc: &mut acc,
                            trades: &mut trades,
                            fixed_realized: &mut fixed_realized,
                            fixed_qty: &mut fixed_qty,
                            collect_curves,
                        },
                    )?;
                    cur_pos = Side::Flat;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                    pos_stop_loss = None;
                    pos_trail_extreme = None;
                    pos_trail_stop = None;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
                    // 止损是风险覆盖, 不是策略信号: 平仓后把目标重置为 Flat,
                    // 并把 stop 之前已经排队的陈旧目标全部改成 Flat。**不能
                    // clear**: 队列长度编码了 delay 的时序, 清空会让下一根
                    // 新信号提前成交; 用 Flat 占位既保持长度, 又不会重入。
                    // 只有止损 bar 及之后新出现的 Buy/Sell 才会重新入场。
                    target = Side::Flat;
                    for slot in &mut pending {
                        *slot = Side::Flat;
                    }
                }
            }
        }};
    }

    // 旧实现把 `cur_pos`/`cash` 在**信号 bar** 就改掉、却用 `i + delay` 的价格成交,
    // 于是权益曲线在成交前就按未来价格盯市 —— Sharpe / 回撤 / Sortino 全部被污染。
    // 现在所有状态变更都推迟到 fill bar, 曲线在 bar i 只反映 bar i 及之前已发生的成交。
    for i in 0..n {
        // 0) `fill_close=true` 时, 到期信号在收盘才成交; 盘中 stop 可能先触发。
        //    先查 stop, 再执行收盘信号, 避免用收盘价"跳过"盘中已经发生的止损。
        if exec.fill_close {
            try_stop_exit!(i);
        }

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
                        fixed_qty,
                        trade_mfe,
                        trade_mae,
                        fee_rate: exec.fee_rate,
                        reason: ExitReason::Signal,
                    };
                    close_at(
                        cargs,
                        &mut CloseState {
                            cash: &mut cash,
                            acc: &mut acc,
                            trades: &mut trades,
                            fixed_realized: &mut fixed_realized,
                            fixed_qty: &mut fixed_qty,
                            collect_curves,
                        },
                    )?;
                    cur_pos = Side::Flat;
                    trade_mfe = 0.0;
                    trade_mae = 0.0;
                    pos_stop_loss = None;
                    pos_trail_extreme = None;
                    pos_trail_stop = None;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
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
                } else if let Some(entry_price) = fill_at(i, due, close, open, exec)
                    && let Some((qty, fixed_qty_entry)) = entry_qty(
                        sizing,
                        &acc,
                        sizing_atr.as_deref(),
                        i,
                        exec.fill_close,
                        entry_price,
                        cash,
                        initial_cash,
                    )?
                {
                    let fee = qty.mul_price(entry_price)?.mul_ratio(exec.fee_rate)?;
                    pos_entry = entry_price;
                    pos_entry_time = ts(i);
                    pos_entry_bar = i;
                    pos_qty = qty;
                    fixed_qty = fixed_qty_entry;
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
                    // 收盘成交时, 本根 high/low 发生在入场之前, 不得回溯触发止损。
                    pos_entry_at_close = exec.fill_close || open.get(i).is_none();
                    let pct_stop = |pct: f64| {
                        if due == Side::Long {
                            entry_price * (1.0 - pct / 100.0)
                        } else {
                            entry_price * (1.0 + pct / 100.0)
                        }
                    };
                    let atr_entry = risk.atr.and_then(|_| {
                        atr_values
                            .as_deref()
                            .and_then(|atr| atr_at_entry(atr, i, exec.fill_close))
                    });
                    let atr_stop = |cfg: AtrStopConfig| {
                        atr_entry.map(|atr| {
                            if due == Side::Long {
                                entry_price - cfg.mult * atr
                            } else {
                                entry_price + cfg.mult * atr
                            }
                        })
                    };
                    // 固定止损: 百分比与 ATR 固定止损取更紧的一侧。
                    let atr_fixed = risk.atr.filter(|c| !c.trailing).and_then(atr_stop);
                    pos_stop_loss = tighter_stop(due, risk.stop_loss_pct.map(pct_stop), atr_fixed);
                    // 追踪止损: 百分比与 ATR 追踪止损共用同一个有利极值。
                    let atr_trail = risk.atr.filter(|c| c.trailing).and_then(atr_stop);
                    let trail = tighter_stop(due, risk.trailing_stop_pct.map(pct_stop), atr_trail);
                    // 即使初始 ATR 尚不可用, 也要先记录极值; 后续 bar 的 ATR
                    // 一旦可用, 追踪止损即可启动。
                    pos_trail_extreme = (risk.trailing_stop_pct.is_some()
                        || risk.atr.is_some_and(|c| c.trailing))
                    .then_some(entry_price);
                    pos_trail_stop = trail;
                    pos_strategy_stop = None;
                    pos_breakeven_stop = None;
                }
            }
        }

        // `fill_close=false` 时, 到期信号已在开盘成交; 新仓在**同一根 bar**
        // 内就可能触发止损(开盘成交, 盘中 high/low 尚未走完)。
        if !exec.fill_close {
            try_stop_exit!(i);
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

        // 3b) 追踪止损只用**已收盘 bar** 的 high/low 更新 extreme, 供下一根
        //     bar 判定; 当根入场且为收盘成交时跳过, 避免把入场前的行情算进去。
        let trail_atr = risk.atr.filter(|c| c.trailing);
        if cur_pos.is_open() && (risk.trailing_stop_pct.is_some() || trail_atr.is_some()) {
            if !(i == pos_entry_bar && pos_entry_at_close) {
                let h = high.get(i).copied().unwrap_or(close[i]);
                let l = low.get(i).copied().unwrap_or(close[i]);
                if let Some(ext) = pos_trail_extreme {
                    let next = match cur_pos {
                        Side::Long if h.is_finite() => ext.max(h),
                        Side::Short if l.is_finite() => ext.min(l),
                        _ => ext,
                    };
                    pos_trail_extreme = Some(next);
                }
            }
            if let Some(ext) = pos_trail_extreme {
                let pct_stop = risk.trailing_stop_pct.map(|pct| {
                    if cur_pos == Side::Long {
                        ext * (1.0 - pct / 100.0)
                    } else {
                        ext * (1.0 + pct / 100.0)
                    }
                });
                let atr_stop = trail_atr.and_then(|cfg| {
                    let atr = atr_values
                        .as_ref()
                        .and_then(|values| values.get(i))
                        .copied()
                        .filter(|v| v.is_finite() && *v > 0.0)?;
                    Some(if cur_pos == Side::Long {
                        ext - cfg.mult * atr
                    } else {
                        ext + cfg.mult * atr
                    })
                });
                // 追踪止损只能收紧: ATR 放大时 `extreme - mult*ATR` 可能比
                // 上一根更松, 必须与已有止损取更紧的一侧, 否则止损会向后
                // 退让。百分比追踪本身单调, 与旧值合并不改变结果。
                let candidate = tighter_stop(cur_pos, pct_stop, atr_stop);
                if let Some(stop) = tighter_stop(cur_pos, pos_trail_stop, candidate) {
                    pos_trail_stop = Some(stop);
                }
            }
        }

        // 3c) 保本止损: 收盘价有利幅度达到 trigger 后, 把止损移到入场价
        //     加减 offset。收盘后更新, 下一根 bar 才生效, 不使用当根 high/low。
        if cur_pos.is_open()
            && let Some(be) = risk.breakeven
            && !(i == pos_entry_bar && pos_entry_at_close)
            && pos_entry.is_finite()
            && pos_entry > 0.0
            && close[i].is_finite()
        {
            let favorable_pct = if cur_pos == Side::Long {
                (close[i] - pos_entry) / pos_entry * 100.0
            } else {
                (pos_entry - close[i]) / pos_entry * 100.0
            };
            if favorable_pct >= be.trigger_pct {
                let candidate = if cur_pos == Side::Long {
                    pos_entry * (1.0 + be.offset_pct / 100.0)
                } else {
                    pos_entry * (1.0 - be.offset_pct / 100.0)
                };
                pos_breakeven_stop = tighter_stop(cur_pos, pos_breakeven_stop, Some(candidate));
            }
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
                fixed_qty,
                trade_mfe,
                trade_mae,
                fee_rate: exec.fee_rate,
                reason: ExitReason::EndOfData,
            };
            close_at(
                cargs,
                &mut CloseState {
                    cash: &mut cash,
                    acc: &mut acc,
                    trades: &mut trades,
                    fixed_realized: &mut fixed_realized,
                    fixed_qty: &mut fixed_qty,
                    collect_curves,
                },
            )?;
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
