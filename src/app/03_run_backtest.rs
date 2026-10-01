use rayon::prelude::*;

use crate::strategies::Signal;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SummaryMetrics {
    pub total_return_pct: f64,
    pub total_return: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub max_drawdown_pct: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub calmar_ratio: f64,
    pub num_trades: u32,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub expectancy: f64,
    pub consecutive_wins: u32,
    pub consecutive_losses: u32,
    pub final_equity: f64,
    pub volatility: f64,
    pub recovery_factor: f64,
    pub avg_hold_bars: f64,
    pub avg_mae: f64,
    pub avg_mfe: f64,
    pub edge_ratio: f64,
    pub wl_ratio: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub sqn: f64,
    pub pf_stability: f64,
    pub z_score: f64,
    pub p_value: f64,
    pub total_fixed_pnl: f64,
    /// 年化成交笔数(往返次数)。信号抖动的主要预警指标:
    /// 平均持仓不足 5 根 bar 时, 该值会高到成本必然吞掉全部毛利。
    pub turnover_per_year: f64,
    /// 执行口径(回放用): 信号延迟 bar 数。
    pub delay_bars: u32,
    /// 执行口径(回放用): 单边佣金 bp。
    pub fee_bps: f64,
    /// 执行口径(回放用): 单边逆向滑点 bp。
    pub slippage_bps: f64,
}

/// 成交执行假设。`delay_bars = 0` 为旧口径(信号当根收盘成交, 前视乐观);
/// `delay_bars = 1` 为推荐口径(信号次根开盘成交)。费率按单边名义扣除。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecConfig {
    pub delay_bars: usize,
    pub fee_rate: f64,
    pub slip_rate: f64,
}

impl Default for ExecConfig {
    /// 全零 = 旧口径, 逐位一致。
    fn default() -> Self {
        Self {
            delay_bars: 0,
            fee_rate: 0.0,
            slip_rate: 0.0,
        }
    }
}

impl ExecConfig {
    #[must_use]
    pub fn from_bps(delay_bars: usize, fee_bps: f64, slippage_bps: f64) -> Self {
        Self {
            delay_bars,
            fee_rate: fee_bps / 10_000.0,
            slip_rate: slippage_bps / 10_000.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub entry_time: i64,
    pub exit_time: i64,
    pub side: i8,
    pub entry_price: f64,
    pub exit_price: f64,
    pub qty: f64,
    pub pnl: f64,
    pub pnl_pct: f64,
    pub fixed_pnl: f64,
    pub bars_held: i64,
    pub max_runup: f64,
    pub max_drawdown: f64,
    pub exit_reason: String,
}

#[derive(Debug, Clone)]
pub struct SimResult {
    pub summary: SummaryMetrics,
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(f64, f64)>,
}

fn signals_to_positions(signals: &[Signal]) -> Vec<i8> {
    let n = signals.len();
    let mut pos = vec![0i8; n];
    let mut cur = 0i8;
    for (i, s) in signals.iter().enumerate() {
        match s {
            Signal::Buy => cur = 1,
            Signal::Sell => cur = -1,
            Signal::Close => cur = 0,
            Signal::Hold => {}
        }
        pos[i] = cur;
    }
    pos
}

struct CloseArgs {
    cur_pos: i8,
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
}

/// 方向 side 的成交价: 买入抬价、卖出压价(逆向滑点)。
fn fill_price(side: i8, base: f64, slip_rate: f64) -> f64 {
    if side == 1 {
        base * (1.0 + slip_rate)
    } else {
        base * (1.0 - slip_rate)
    }
}

/// 信号 bar `i`、订单方向 `side` 的成交位置与价格。
/// - `delay_bars = 0`: 当根收盘(旧口径)。
/// - `delay_bars > 0`: fill bar 的开盘; 开仓 fill bar 超出数据返回 `None`(放弃,
///   无未来可成交), 平仓钳到末根收盘(与旧口径的末根离场一致)。
fn resolve_fill(
    i: usize,
    side: i8,
    is_open: bool,
    close: &[f64],
    open: &[f64],
    n: usize,
    exec: &ExecConfig,
) -> Option<(usize, f64)> {
    if exec.delay_bars == 0 {
        return Some((i, fill_price(side, close[i], exec.slip_rate)));
    }
    let fb = i + exec.delay_bars;
    if fb >= n {
        if is_open {
            return None;
        }
        let last = n - 1;
        return Some((last, fill_price(side, close[last], exec.slip_rate)));
    }
    let base = open.get(fb).copied().unwrap_or(close[fb]);
    Some((fb, fill_price(side, base, exec.slip_rate)))
}

fn close_position(args: CloseArgs) -> (f64, Trade) {
    let CloseArgs {
        cur_pos,
        pos_qty,
        pos_entry,
        pos_entry_time,
        pos_entry_bar,
        exit_price,
        exit_time,
        exit_bar,
        initial_cash,
        trade_mfe,
        trade_mae,
        fee_rate,
    } = args;

    // 平仓单方向与持仓反向: 多头平仓为卖, 空头平仓为买; 佣金按单边名义扣除。
    let exit_fee = exit_price * pos_qty * fee_rate;
    let (cash_delta, pnl) = if cur_pos == 1 {
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
    let fixed_pnl = if cur_pos == 1 {
        (exit_price - pos_entry) * fixed_qty - entry_fee_fixed - exit_fee_fixed
    } else {
        (pos_entry - exit_price) * fixed_qty - entry_fee_fixed - exit_fee_fixed
    };
    let pnl_pct = if cur_pos == 1 {
        (exit_price - pos_entry) / pos_entry
    } else {
        (pos_entry - exit_price) / pos_entry
    };
    let trade = Trade {
        entry_time: pos_entry_time,
        exit_time,
        side: cur_pos,
        entry_price: pos_entry,
        exit_price,
        qty: pos_qty,
        pnl,
        pnl_pct,
        fixed_pnl,
        bars_held: (exit_bar as i64) - (pos_entry_bar as i64),
        max_runup: trade_mfe,
        max_drawdown: trade_mae,
        exit_reason: "signal".to_string(),
    };
    (cash_delta, trade)
}

fn scan_trades(
    close: &[f64],
    open: &[f64],
    positions: &[i8],
    time: &[i64],
    initial_cash: f64,
    exec: &ExecConfig,
) -> (Vec<Trade>, f64, Vec<f64>, Vec<f64>) {
    let n = close.len().min(positions.len()).min(open.len());
    let ts = |i: usize| if i < time.len() { time[i] } else { i as i64 };
    let mut cash = initial_cash;
    let mut pos_qty = 0.0;
    let mut pos_entry = 0.0;
    let mut pos_entry_time: i64 = 0;
    let mut pos_entry_bar: usize = 0;
    let mut cur_pos: i8 = 0;
    let mut trades = Vec::with_capacity(1024);
    let mut trade_pnls = Vec::with_capacity(1024);
    let mut trade_pnl_pcts = Vec::with_capacity(1024);
    let mut equity_curve = Vec::with_capacity(n);

    let mut trade_mfe: f64 = 0.0;
    let mut trade_mae: f64 = 0.0;

    for i in 0..n {
        // 浮动盈亏只统计已成交持仓(fill bar 之后), fill 前的漂移不计入 MFE/MAE。
        if cur_pos != 0 && i >= pos_entry_bar {
            let favor = if cur_pos == 1 {
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

        if cur_pos != 0 && cur_pos != positions[i] {
            let exit_side: i8 = -cur_pos;
            // 平仓 resolve 永不返回 None(尾部钳到末根), 直接解包。
            let (fb, exit_price) = resolve_fill(i, exit_side, false, close, open, n, exec)
                .expect("close fill is clamped to last bar");
            let (cash_delta, trade) = close_position(CloseArgs {
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
            });
            cash += cash_delta;
            trade_pnls.push(trade.pnl);
            trade_pnl_pcts.push(trade.pnl_pct);
            trades.push(trade);
            cur_pos = 0;
            trade_mfe = 0.0;
            trade_mae = 0.0;
        }

        if cur_pos == 0 && positions[i] != 0 {
            let side = positions[i];
            if let Some((fb, entry_price)) = resolve_fill(i, side, true, close, open, n, exec) {
                let qty = cash / entry_price;
                let fee = entry_price * qty * exec.fee_rate;
                pos_entry = entry_price;
                pos_entry_time = ts(fb);
                pos_entry_bar = fb;
                pos_qty = qty;
                if side == 1 {
                    cash -= qty * entry_price + fee;
                } else {
                    cash += qty * entry_price - fee;
                }
                cur_pos = side;
                trade_mfe = 0.0;
                trade_mae = 0.0;
            }
        }

        let eq = if cur_pos == 1 {
            cash + pos_qty * close[i]
        } else if cur_pos == -1 {
            cash - pos_qty * close[i]
        } else {
            cash
        };
        equity_curve.push(eq);
    }

    if cur_pos != 0 {
        let last_price = fill_price(-cur_pos, close[n - 1], exec.slip_rate);
        let (cash_delta, trade) = close_position(CloseArgs {
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
        });
        cash += cash_delta;
        trade_pnls.push(trade.pnl);
        trade_pnl_pcts.push(trade.pnl_pct);
        trades.push(trade);
        equity_curve[n - 1] = cash;
    }

    (trades, cash, equity_curve, trade_pnl_pcts)
}

fn compute_metrics(
    equity_curve: &[f64],
    trades: &[Trade],
    trade_pnls: &[f64],
    trade_pnl_pcts: &[f64],
    initial_cash: f64,
    bars_per_year: f64,
) -> SummaryMetrics {
    let n = equity_curve.len();
    if n < 2 {
        return SummaryMetrics::default();
    }

    let final_eq = *equity_curve.last().unwrap_or(&initial_cash);
    let total_return = (final_eq - initial_cash) / initial_cash;

    let mut ret_mean = 0.0;
    let mut ret_m2 = 0.0;
    let mut ret_m2_downside = 0.0;
    let mut peak = initial_cash;
    let mut max_dd = 0.0;

    for i in 1..n {
        let prev = equity_curve[i - 1];
        let curr = equity_curve[i];
        let ret = (curr / prev).ln();

        let delta = ret - ret_mean;
        ret_mean += delta / i as f64;
        let delta2 = ret - ret_mean;
        ret_m2 += delta * delta2;
        if ret < 0.0 {
            ret_m2_downside += ret * ret;
        }

        if curr > peak {
            peak = curr;
        }
        let dd = (peak - curr) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    let num_bars = (n - 1) as f64;
    let years = if bars_per_year > 0.0 {
        num_bars / bars_per_year
    } else {
        0.0
    };
    let sharpe = if ret_m2 > 0.0 && num_bars > 1.0 {
        let variance = ret_m2 / (num_bars - 1.0);
        ret_mean / variance.sqrt() * bars_per_year.sqrt()
    } else {
        0.0
    };

    let sortino = if ret_m2_downside > 0.0 && num_bars > 1.0 {
        let downside_var = ret_m2_downside / num_bars;
        ret_mean / downside_var.sqrt() * bars_per_year.sqrt()
    } else {
        0.0
    };

    let volatility = if ret_m2 > 0.0 && num_bars > 1.0 {
        (ret_m2 / (num_bars - 1.0)).sqrt() * bars_per_year.sqrt() * 100.0
    } else {
        0.0
    };

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

    let num_trades = trade_pnls.len() as u32;
    let mut num_wins = 0u32;
    let mut num_losses = 0u32;
    let mut total_win = 0.0;
    let mut total_loss = 0.0;
    let mut cw = 0u32;
    let mut cl = 0u32;
    let mut max_cw = 0u32;
    let mut max_cl = 0u32;

    let mut sum_mae = 0.0;
    let mut sum_mfe = 0.0;
    let mut sum_hold = 0.0;
    let mut total_fixed_pnl = 0.0;
    let mut pnl_m2 = 0.0;
    let mut pnl_m3 = 0.0;
    let mut pnl_m4 = 0.0;
    let mut runs = 0u32;
    let mut prev_win: Option<bool> = None;

    let nt = num_trades as f64;
    let pnl_mean = if nt > 0.0 {
        trade_pnls.iter().sum::<f64>() / nt
    } else {
        0.0
    };

    for (i, &p) in trade_pnls.iter().enumerate() {
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

        if i < trades.len() {
            sum_mae += trades[i].max_drawdown;
            sum_mfe += trades[i].max_runup;
            sum_hold += trades[i].bars_held as f64;
            total_fixed_pnl += trades[i].fixed_pnl;
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
        trade_pnl_pcts.iter().sum::<f64>() / num_trades as f64
    } else {
        0.0
    };

    let avg_mae = if nt > 0.0 { sum_mae / nt } else { 0.0 };
    let avg_mfe = if nt > 0.0 { sum_mfe / nt } else { 0.0 };
    let edge_ratio = if avg_mae > 0.0 {
        avg_mfe / avg_mae
    } else {
        0.0
    };
    let avg_hold_bars = if nt > 0.0 { sum_hold / nt } else { 0.0 };

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
            let effective_n = nt.min(100.0);
            pnl_mean / pnl_std * effective_n.sqrt()
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
                for &p in &trade_pnls[..half] {
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
                for &p in &trade_pnls[half..] {
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
        total_fixed_pnl,
        turnover_per_year: if years > 0.0 {
            num_trades as f64 / years
        } else {
            0.0
        },
        // 执行口径由 simulate()/落盘层回填, 裸 compute 为旧口径 0。
        delay_bars: 0,
        fee_bps: 0.0,
        slippage_bps: 0.0,
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
    let positions = signals_to_positions(signals);
    let (trades, _final_cash, equity_curve, trade_pnl_pcts) =
        scan_trades(close, open, &positions, &[], initial_cash, exec);
    let mut summary = compute_metrics(
        &equity_curve,
        &trades,
        &trades.iter().map(|t| t.pnl).collect::<Vec<_>>(),
        &trade_pnl_pcts,
        initial_cash,
        bars_per_year,
    );
    summary.delay_bars = exec.delay_bars as u32;
    summary.fee_bps = exec.fee_rate * 10_000.0;
    summary.slippage_bps = exec.slip_rate * 10_000.0;
    summary
}

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
        };
    }
    let positions = signals_to_positions(signals);
    let (trades, _final_cash, equity_curve, trade_pnl_pcts) =
        scan_trades(close, open, &positions, time, initial_cash, exec);
    let mut summary = compute_metrics(
        &equity_curve,
        &trades,
        &trades.iter().map(|t| t.pnl).collect::<Vec<_>>(),
        &trade_pnl_pcts,
        initial_cash,
        bars_per_year,
    );
    summary.delay_bars = exec.delay_bars as u32;
    summary.fee_bps = exec.fee_rate * 10_000.0;
    summary.slippage_bps = exec.slip_rate * 10_000.0;

    let equity_curve: Vec<(f64, f64)> = equity_curve
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
        trades,
        equity_curve,
    }
}

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
    fn test_default_exec_matches_legacy_close_fill() {
        // 默认口径必须与旧逻辑逐位一致: 单多信号当根收盘进、末根收盘出。
        let close = vec![100.0, 102.0, 101.0, 105.0];
        let open = vec![99.0, 100.5, 102.5, 101.5];
        let sig = vec![Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &ExecConfig::default());
        assert_eq!(m.num_trades, 1);
        // 固定名义 1000/100=10 股, (105-100)*10=50。
        assert!(
            (m.total_fixed_pnl - 50.0).abs() < 1e-9,
            "{}",
            m.total_fixed_pnl
        );
        assert_eq!(m.delay_bars, 0);
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
    fn test_fee_and_slippage_deducted_per_side() {
        // fee 10bp + slip 10bp 双边: 进出各扣一次。用 Close 离场以隔离单笔数学。
        let close = vec![100.0, 110.0];
        let open = close.clone();
        let sig = vec![Signal::Buy, Signal::Close];
        let exec = ExecConfig::from_bps(0, 10.0, 10.0);
        let m = simulate(&close, &open, &sig, 1000.0, 252.0, &exec);
        assert_eq!(m.num_trades, 1);
        // 进 100*1.001=100.1, 出 110*0.999=109.89; 名义 1000/100.1;
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
}
