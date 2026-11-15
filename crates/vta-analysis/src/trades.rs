//! 交易结构统计: 方向、离场原因、持仓期、盈亏分布。

use std::collections::BTreeMap;

use vta_core::engine::Side;

use serde::Serialize;

use vta_report::TradeRecord;

use crate::stats::{median, quantile_sorted};

/// 一个计数项。
#[derive(Debug, Clone, Serialize)]
pub struct CountItem {
    /// 名称。
    pub name: String,
    /// 次数。
    pub count: usize,
}

/// 单组合的交易结构。
#[derive(Debug, Clone, Serialize)]
pub struct TradeStats {
    /// 总笔数。
    pub n: usize,
    /// 多头笔数。
    pub long: usize,
    /// 空头笔数。
    pub short: usize,
    /// 盈利笔数。
    pub wins: usize,
    /// 亏损笔数。
    pub losses: usize,
    /// 保本笔数。
    pub breakeven: usize,
    /// 胜率(盈利 / 已决笔数, 0~1)。
    pub win_rate: Option<f64>,
    /// 毛盈利(复利腿 pnl)。
    pub gross_profit: f64,
    /// 毛亏损绝对值(复利腿 pnl)。
    pub gross_loss: f64,
    /// profit factor。
    pub profit_factor: Option<f64>,
    /// 逐笔 pnl_pct 均值。
    pub avg_pnl_pct: Option<f64>,
    /// 逐笔 pnl_pct 中位数。
    pub median_pnl_pct: Option<f64>,
    /// 逐笔 fixed_pnl 均值。
    pub avg_fixed_pnl: Option<f64>,
    /// 逐笔 fixed_pnl 中位数。
    pub median_fixed_pnl: Option<f64>,
    /// 最好的一笔 fixed_pnl。
    pub best_fixed_pnl: Option<f64>,
    /// 最差的一笔 fixed_pnl。
    pub worst_fixed_pnl: Option<f64>,
    /// 平均持仓 bar 数。
    pub avg_bars_held: Option<f64>,
    /// 持仓 bar 数中位数。
    pub median_bars_held: Option<f64>,
    /// 平均 MAE(价格差)。
    pub avg_mae: Option<f64>,
    /// 平均 MFE(价格差)。
    pub avg_mfe: Option<f64>,
    /// `avg_mfe / avg_mae`。
    pub edge_ratio: Option<f64>,
    /// 多头平均 pnl_pct。
    pub avg_long_pnl_pct: Option<f64>,
    /// 空头平均 pnl_pct。
    pub avg_short_pnl_pct: Option<f64>,
    /// 多头胜率。
    pub long_win_rate: Option<f64>,
    /// 空头胜率。
    pub short_win_rate: Option<f64>,
    /// 离场原因计数(按次数降序)。
    pub exit_reasons: Vec<CountItem>,
}

fn avg_by(x: &[f64]) -> Option<f64> {
    if x.is_empty() {
        None
    } else {
        Some(x.iter().sum::<f64>() / x.len() as f64)
    }
}

fn win_rate(wins: usize, losses: usize) -> Option<f64> {
    let decided = wins + losses;
    (decided > 0).then(|| wins as f64 / decided as f64)
}

/// 汇总逐笔成交。
pub(crate) fn trade_stats(trades: &[TradeRecord]) -> TradeStats {
    let n = trades.len();
    let mut long = 0usize;
    let mut short = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut breakeven = 0usize;
    let mut gross_profit = 0.0;
    let mut gross_loss = 0.0;
    let mut pnl_pcts = Vec::with_capacity(n);
    let mut fixed_pnls = Vec::with_capacity(n);
    let mut holds = Vec::with_capacity(n);
    let mut maes = Vec::with_capacity(n);
    let mut mfes = Vec::with_capacity(n);
    let mut long_pcts = Vec::new();
    let mut short_pcts = Vec::new();
    let mut long_wins = 0usize;
    let mut long_losses = 0usize;
    let mut short_wins = 0usize;
    let mut short_losses = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();

    for t in trades {
        match t.side {
            Side::Long => {
                long += 1;
                long_pcts.push(t.pnl_pct);
            }
            Side::Short => {
                short += 1;
                short_pcts.push(t.pnl_pct);
            }
            Side::Flat => {}
        }
        let pnl = t.pnl.to_f64();
        let fixed = t.fixed_pnl.to_f64();
        if pnl > 0.0 {
            wins += 1;
            gross_profit += pnl;
            if t.side == Side::Long {
                long_wins += 1;
            } else if t.side == Side::Short {
                short_wins += 1;
            }
        } else if pnl < 0.0 {
            losses += 1;
            gross_loss += -pnl;
            if t.side == Side::Long {
                long_losses += 1;
            } else if t.side == Side::Short {
                short_losses += 1;
            }
        } else {
            breakeven += 1;
        }
        pnl_pcts.push(t.pnl_pct);
        fixed_pnls.push(fixed);
        holds.push(t.bars_held as f64);
        maes.push(t.max_drawdown);
        mfes.push(t.max_runup);
        *reasons.entry(t.exit_reason.clone()).or_insert(0) += 1;
    }

    let mut exit_reasons: Vec<CountItem> = reasons
        .into_iter()
        .map(|(name, count)| CountItem { name, count })
        .collect();
    exit_reasons.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

    let avg_mae = avg_by(&maes);
    let avg_mfe = avg_by(&mfes);
    let edge_ratio = match (avg_mfe, avg_mae) {
        (Some(mfe), Some(mae)) if mae > 0.0 => Some(mfe / mae),
        _ => None,
    };
    let mut sorted_fixed = fixed_pnls.clone();
    sorted_fixed.sort_by(f64::total_cmp);
    let median_fixed_pnl = (!sorted_fixed.is_empty()).then(|| quantile_sorted(&sorted_fixed, 0.5));
    let best_fixed_pnl = sorted_fixed.last().copied();
    let worst_fixed_pnl = sorted_fixed.first().copied();

    TradeStats {
        n,
        long,
        short,
        wins,
        losses,
        breakeven,
        win_rate: win_rate(wins, losses),
        gross_profit,
        gross_loss,
        profit_factor: if gross_loss > 0.0 {
            Some(gross_profit / gross_loss)
        } else {
            Some(gross_profit.max(0.0))
        },
        avg_pnl_pct: avg_by(&pnl_pcts),
        median_pnl_pct: median(&pnl_pcts),
        avg_fixed_pnl: avg_by(&fixed_pnls),
        median_fixed_pnl,
        best_fixed_pnl,
        worst_fixed_pnl,
        avg_bars_held: avg_by(&holds),
        median_bars_held: median(&holds),
        avg_mae,
        avg_mfe,
        edge_ratio,
        avg_long_pnl_pct: avg_by(&long_pcts),
        avg_short_pnl_pct: avg_by(&short_pcts),
        long_win_rate: win_rate(long_wins, long_losses),
        short_win_rate: win_rate(short_wins, short_losses),
        exit_reasons,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use vta_core::money::Amount;

    fn tr(side: Side, pnl: f64, fixed: f64, reason: &str) -> TradeRecord {
        TradeRecord {
            entry_time: 0,
            exit_time: 1,
            side,
            entry_price: 100.0,
            exit_price: 101.0,
            qty: Amount::from_i64(1),
            pnl: Amount::from_f64(pnl).unwrap(),
            pnl_pct: 0.01,
            fixed_pnl: Amount::from_f64(fixed).unwrap(),
            bars_held: 3,
            max_runup: 2.0,
            max_drawdown: 1.0,
            exit_reason: reason.into(),
        }
    }

    #[test]
    fn counts_sides_and_reasons() {
        let s = trade_stats(&[
            tr(Side::Long, 10.0, 10.0, "signal"),
            tr(Side::Short, -5.0, -5.0, "stop_loss"),
            tr(Side::Long, 0.0, 0.0, "signal"),
        ]);
        assert_eq!(s.n, 3);
        assert_eq!((s.long, s.short), (2, 1));
        assert_eq!((s.wins, s.losses, s.breakeven), (1, 1, 1));
        assert!((s.win_rate.unwrap() - 0.5).abs() < 1e-12);
        assert!((s.profit_factor.unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(s.exit_reasons[0].name, "signal");
        assert_eq!(s.exit_reasons[0].count, 2);
    }
}
