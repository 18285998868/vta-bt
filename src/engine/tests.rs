//! `engine` 的单元测试。

use std::num::NonZeroUsize;

use crate::strategies::{Signal, StopLines};

use super::accum::*;
use super::exec::*;
use super::metrics::*;
use super::scan::*;
use super::sizing::*;
use super::stops::*;
use super::types::*;
use super::*;

fn make_prices(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 100.0 + (i as f64 * 0.1).sin() * 20.0)
        .collect()
}

/// 固定 1 万初始资金、无成本、无止损的详细回测, 仅改变仓位模式。
fn sized_detail(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    open: &[f64],
    signals: &[Signal],
    sizing: SizingConfig,
) -> SimResult {
    simulate_detailed_ohlc_with_stops_and_sizing(
        &[],
        high,
        low,
        close,
        open,
        signals,
        None,
        crate::money::Amount::from_i64(10_000),
        365.0,
        &ExecConfig::default(),
        &StopConfig::default(),
        &sizing,
    )
    .expect("合法回测必须成功")
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
        buy_hold_pnl(&[], crate::money::Amount::from_i64(1000)).expect("非法输入应返回零, 不报错"),
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

/// 固定仓位: 复利腿随当前现金增长, fixed 腿始终按初始资金计价。
#[test]
fn fixed_sizing_compounds_main_leg_but_not_fixed_leg() {
    // 两笔各 +10%: AllIn 10000→12100; 50% 仓位 10000→11025;
    // fixed 腿每笔名义 5000, 两笔各 +500 → 11000。
    let close = vec![100.0, 100.0, 110.0, 110.0, 110.0, 121.0, 121.0];
    let open = close.clone();
    let mut sig = vec![Signal::Hold; close.len()];
    sig[0] = Signal::Buy;
    sig[2] = Signal::Close;
    sig[3] = Signal::Buy;
    sig[5] = Signal::Close;
    let out = sized_detail(
        &close,
        &close,
        &close,
        &open,
        &sig,
        SizingConfig::Fixed { fraction_pct: 50.0 },
    );
    assert_eq!(out.summary.num_trades, 2);
    assert!(
        (out.summary.final_equity.to_f64() - 11_025.0).abs() < 1e-9,
        "{}",
        out.summary.final_equity
    );
    assert!(
        (out.summary.total_fixed_pnl.to_f64() - 1_000.0).abs() < 1e-9,
        "{}",
        out.summary.total_fixed_pnl
    );
    let fixed_last = *out.fixed_curve.last().expect("曲线非空");
    assert!((fixed_last - 11_000.0).abs() < 1e-9, "{fixed_last}");
}

/// ATR 仓位: 名义 = 风险预算 / (mult × ATR); 无杠杆模型下截断到 1 倍权益。
#[test]
fn atr_sizing_sizes_by_risk_and_caps_at_no_leverage() {
    // 恒定 TR=5 ⇒ ATR(2)=5。risk 1%、2×ATR ⇒ 名义 10% ⇒ 10 张。
    let close = vec![100.0; 8];
    let high = vec![102.5; 8];
    let low = vec![97.5; 8];
    let open = close.clone();
    let mut sig = vec![Signal::Hold; 8];
    sig[2] = Signal::Buy;
    sig[4] = Signal::Close;
    let out = sized_detail(
        &high,
        &low,
        &close,
        &open,
        &sig,
        SizingConfig::Atr {
            risk_pct: 1.0,
            period: 2,
            mult: 2.0,
            max_position_pct: 100.0,
            fallback_fraction_pct: 0.0,
        },
    );
    assert_eq!(out.trades.len(), 1);
    assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(10));

    // risk 100%、1×ATR ⇒ 名义 20 倍权益, 但引擎无杠杆, 截断到 1 倍。
    let out = sized_detail(
        &high,
        &low,
        &close,
        &open,
        &sig,
        SizingConfig::Atr {
            risk_pct: 100.0,
            period: 2,
            mult: 1.0,
            max_position_pct: 100.0,
            fallback_fraction_pct: 0.0,
        },
    );
    assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(100));
}

/// ATR 未就绪时按 fallback 比例开仓; fallback=0 则继续空仓等待。
#[test]
fn atr_sizing_uses_fallback_until_atr_is_ready() {
    let close = vec![100.0; 8];
    let high = vec![102.5; 8];
    let low = vec![97.5; 8];
    let open = close.clone();
    let mut sig = vec![Signal::Hold; 8];
    sig[1] = Signal::Buy;
    sig[3] = Signal::Close;
    let out = sized_detail(
        &high,
        &low,
        &close,
        &open,
        &sig,
        SizingConfig::Atr {
            risk_pct: 1.0,
            period: 10,
            mult: 2.0,
            max_position_pct: 100.0,
            fallback_fraction_pct: 20.0,
        },
    );
    assert_eq!(out.trades.len(), 1);
    assert_eq!(out.trades[0].qty, crate::money::Amount::from_i64(20));
}

/// Kelly: 样本不足走 fallback; 样本足够后按最近 N 笔的胜率/赔率折减。
#[test]
fn kelly_uses_recent_trade_stats_after_min_trades() {
    let close = vec![
        100.0, 100.0, 110.0, 110.0, 110.0, 104.5, 104.5, 104.5, 104.5, 104.5,
    ];
    let open = close.clone();
    let mut sig = vec![Signal::Hold; close.len()];
    sig[0] = Signal::Buy;
    sig[2] = Signal::Close;
    sig[3] = Signal::Buy;
    sig[5] = Signal::Close;
    sig[6] = Signal::Buy;
    sig[8] = Signal::Close;
    let out = sized_detail(
        &close,
        &close,
        &close,
        &open,
        &sig,
        SizingConfig::Kelly {
            lookback: 2,
            min_trades: 2,
            kelly_fraction: 0.5,
            max_position_pct: 100.0,
            fallback_fraction_pct: 100.0,
        },
    );
    assert_eq!(out.trades.len(), 3);
    // 前两笔 fallback 全仓: 10000→11000→10450; 第三笔半 Kelly:
    // p=0.5, b=2 ⇒ f=0.25, 半 Kelly=0.125 ⇒ 10450×0.125/104.5 = 12.5 张。
    assert_eq!(
        out.trades[2].qty,
        crate::money::Amount::from_f64(12.5).expect("12.5 合法")
    );
}

/// Kelly 估计出非正优势时不开仓(而不是回退到 fallback)。
#[test]
fn kelly_with_non_positive_edge_skips_entry() {
    let close = vec![
        100.0, 100.0, 110.0, 110.0, 110.0, 99.0, 99.0, 99.0, 99.0, 99.0,
    ];
    let open = close.clone();
    let mut sig = vec![Signal::Hold; close.len()];
    sig[0] = Signal::Buy;
    sig[2] = Signal::Close;
    sig[3] = Signal::Buy;
    sig[5] = Signal::Close;
    sig[6] = Signal::Buy;
    sig[8] = Signal::Close;
    let out = sized_detail(
        &close,
        &close,
        &close,
        &open,
        &sig,
        SizingConfig::Kelly {
            lookback: 2,
            min_trades: 2,
            kelly_fraction: 0.5,
            max_position_pct: 100.0,
            fallback_fraction_pct: 100.0,
        },
    );
    assert_eq!(out.summary.num_trades, 2, "f<=0 时第三笔不得开仓");
}

/// 平仓必须复用开仓时存的 fixed 腿数量, 不能按 `initial_cash` 重算。
#[test]
fn close_outcome_uses_stored_fixed_qty() {
    let out = close_outcome(CloseArgs {
        cur_pos: Side::Long,
        pos_qty: crate::money::Amount::from_i64(10),
        pos_entry: 100.0,
        pos_entry_time: 0,
        pos_entry_bar: 0,
        exit_price: 110.0,
        exit_time: 1,
        exit_bar: 1,
        // 故意与复利腿不同: 若平仓时按 initial_cash 重算, 这里会得到 100 而非 250。
        fixed_qty: crate::money::Amount::from_i64(25),
        trade_mfe: 0.0,
        trade_mae: 0.0,
        fee_rate: 0.0,
        reason: ExitReason::Signal,
    })
    .expect("合法平仓");
    assert_eq!(out.pnl, crate::money::Amount::from_i64(100));
    assert_eq!(out.fixed_pnl, crate::money::Amount::from_i64(250));
}

/// `params` 只能有一个 `;` 分隔符: `params_to_json` 只按首个 `;` 切分。
#[test]
fn apply_to_appends_stop_and_sizing_after_one_separator() {
    let base = "period=20";
    let no_stop = StopConfig::default();
    assert_eq!(no_stop.apply_to(base, &SizingConfig::AllIn), base);
    assert_eq!(
        no_stop.apply_to(base, &SizingConfig::Fixed { fraction_pct: 20.0 }),
        "period=20;size=fixed,size_fraction_pct=20"
    );

    let stop = StopConfig {
        stop_loss_pct: Some(2.0),
        ..StopConfig::default()
    };
    assert_eq!(
        stop.apply_to(base, &SizingConfig::AllIn),
        "period=20;stop_loss_pct=2"
    );
    assert_eq!(
        stop.apply_to(base, &SizingConfig::Fixed { fraction_pct: 20.0 }),
        "period=20;stop_loss_pct=2,size=fixed,size_fraction_pct=20"
    );
    for sizing in [
        SizingConfig::AllIn,
        SizingConfig::Fixed { fraction_pct: 20.0 },
    ] {
        assert_eq!(
            stop.apply_to(base, &sizing).matches(';').count(),
            1,
            "stop+sizing 只能有一个分隔符"
        );
    }
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
        &close,
        &close,
        &sig,
        None,
        &[],
        crate::money::Amount::from_i64(10_000),
        &exec,
        &StopConfig::default(),
        &SizingConfig::default(),
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

/// 构造带止损的详细回测; 默认零费率、零滑点、`delay=1`。
fn run_stop(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    signals: &[Signal],
    stop: StopConfig,
    fill_close: bool,
) -> SimResult {
    let time: Vec<i64> = (0..close.len() as i64).collect();
    let exec = ExecConfig::try_from_bps_fill(1, 0.0, 0.0, fill_close)
        .expect("零费率/零滑点/延迟 1 必须合法");
    simulate_detailed_ohlc(
        &time,
        high,
        low,
        close,
        open,
        signals,
        crate::money::Amount::from_i64(1_000),
        252.0,
        &exec,
        &stop,
    )
    .expect("合法回测必须成功")
}

/// 构造带策略止损线的详细回测; 默认零费率、零滑点、`delay=1`。
#[allow(clippy::too_many_arguments)]
fn run_stops(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    signals: &[Signal],
    stop_lines: Option<&StopLines>,
    stop: StopConfig,
    fill_close: bool,
) -> SimResult {
    let time: Vec<i64> = (0..close.len() as i64).collect();
    let exec = ExecConfig::try_from_bps_fill(1, 0.0, 0.0, fill_close)
        .expect("零费率/零滑点/延迟 1 必须合法");
    simulate_detailed_ohlc_with_stops(
        &time,
        high,
        low,
        close,
        open,
        signals,
        stop_lines,
        crate::money::Amount::from_i64(1_000),
        252.0,
        &exec,
        &stop,
    )
    .expect("合法回测必须成功")
}

#[test]
fn exit_long_and_exit_short_only_close_matching_side() {
    assert_eq!(next_target(Side::Long, Signal::ExitLong), Side::Flat);
    assert_eq!(next_target(Side::Short, Signal::ExitLong), Side::Short);
    assert_eq!(next_target(Side::Short, Signal::ExitShort), Side::Flat);
    assert_eq!(next_target(Side::Long, Signal::ExitShort), Side::Long);

    let open = [100.0; 5];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::ExitLong,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &open,
        &open,
        &open,
        &signals,
        StopConfig::default(),
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].entry_time, 1);
    assert_eq!(d.trades[0].exit_time, 3, "ExitLong 必须延迟一根 bar 成交");
    assert_eq!(d.trades[0].exit_reason, "signal");
}

#[test]
fn strategy_stop_line_long_triggers_intrabar() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 101.0];
    let low = [99.0, 99.0, 95.0, 99.0, 99.0];
    let close = [100.0, 100.0, 96.0, 100.0, 100.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let lines = StopLines {
        long: Some(vec![f64::NAN, 96.0, 96.0, 96.0, 96.0]),
        short: None,
    };
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        Some(&lines),
        StopConfig::default(),
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].side, Side::Long);
    assert_eq!(d.trades[0].exit_time, 2);
    assert!((d.trades[0].exit_price - 96.0).abs() < 1e-12);
    assert_eq!(d.trades[0].exit_reason, "strategy_stop");
    assert_eq!(d.summary.strategy_stop_exits, 1);
    assert_eq!(d.summary.stop_loss_exits, 0);
    assert_eq!(d.summary.trailing_stop_exits, 0);
    assert!((d.summary.final_equity.to_f64() - 960.0).abs() < 1e-9);
}

#[test]
fn strategy_stop_line_short_triggers_intrabar() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 105.0, 101.0, 101.0];
    let low = [99.0, 99.0, 99.0, 99.0, 99.0];
    let close = [100.0, 100.0, 104.0, 100.0, 100.0];
    let signals = [
        Signal::Sell,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let lines = StopLines {
        long: None,
        short: Some(vec![f64::NAN, 104.0, 104.0, 104.0, 104.0]),
    };
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        Some(&lines),
        StopConfig::default(),
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].side, Side::Short);
    assert_eq!(d.trades[0].exit_time, 2);
    assert!((d.trades[0].exit_price - 104.0).abs() < 1e-12);
    assert_eq!(d.trades[0].exit_reason, "strategy_stop");
    assert_eq!(d.summary.strategy_stop_exits, 1);
    assert!((d.summary.final_equity.to_f64() - 960.0).abs() < 1e-9);
}

#[test]
fn strategy_stop_line_gap_fills_at_open() {
    let open = [100.0, 100.0, 94.0, 94.0, 94.0];
    let high = [101.0, 101.0, 95.0, 95.0, 95.0];
    let low = [99.0, 99.0, 93.0, 93.0, 93.0];
    let close = [100.0, 100.0, 94.0, 94.0, 94.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let lines = StopLines {
        long: Some(vec![f64::NAN, 96.0, 96.0, 96.0, 96.0]),
        short: None,
    };
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        Some(&lines),
        StopConfig::default(),
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].exit_time, 2);
    assert!(
        (d.trades[0].exit_price - 94.0).abs() < 1e-12,
        "跳空必须按 open 成交, exit={}",
        d.trades[0].exit_price
    );
    assert_eq!(d.trades[0].exit_reason, "strategy_stop");
}

#[test]
fn strategy_stop_line_is_tighter_than_engine_stop() {
    let open = [100.0; 4];
    let high = [101.0, 101.0, 101.0, 101.0];
    let low = [99.0, 98.5, 99.0, 99.0];
    let close = [100.0; 4];
    let signals = [Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
    let lines = StopLines {
        long: Some(vec![f64::NAN, 99.0, 99.0, 99.0]),
        short: None,
    };
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        Some(&lines),
        StopConfig {
            stop_loss_pct: Some(2.0),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert!((d.trades[0].exit_price - 99.0).abs() < 1e-12);
    assert_eq!(d.trades[0].exit_reason, "strategy_stop");
    assert_eq!(d.summary.strategy_stop_exits, 1);
    assert_eq!(d.summary.stop_loss_exits, 0);
}

#[test]
fn strategy_stop_lines_must_cover_signal_length() {
    let close = [100.0; 3];
    let signals = [Signal::Buy, Signal::Hold, Signal::Hold];
    let lines = StopLines {
        long: Some(vec![99.0]),
        short: None,
    };
    let err = simulate_ohlc_with_stops(
        &close,
        &close,
        &close,
        &close,
        &signals,
        Some(&lines),
        crate::money::Amount::from_i64(1_000),
        252.0,
        &ExecConfig::default(),
        &StopConfig::default(),
    )
    .expect_err("短止损线必须报错");
    assert!(err.to_string().contains("长度不足"), "err={err}");
}

#[test]
fn atr_fixed_stop_uses_prior_completed_bar_atr() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 101.0, 101.0];
    let low = [99.0, 99.0, 99.0, 99.0, 99.0, 95.0];
    let close = [100.0; 6];
    let signals = [
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        None,
        StopConfig {
            atr: Some(AtrStopConfig {
                period: 2,
                mult: 2.0,
                trailing: false,
            }),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].entry_time, 4);
    assert_eq!(d.trades[0].exit_time, 5);
    assert!((d.trades[0].exit_price - 96.0).abs() < 1e-12);
    assert_eq!(d.trades[0].exit_reason, "stop_loss");
    assert_eq!(d.summary.stop_loss_exits, 1);
}

#[test]
fn atr_trailing_stop_ratchets_from_highest_high() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 110.0, 101.0];
    let low = [99.0, 99.0, 99.0, 99.0, 99.0, 90.0];
    let close = [100.0, 100.0, 100.0, 100.0, 105.0, 95.0];
    let signals = [
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        None,
        StopConfig {
            atr: Some(AtrStopConfig {
                period: 2,
                mult: 2.0,
                trailing: true,
            }),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].entry_time, 4);
    // bar4 收盘: extreme=110, ATR=6.5, trail=110-2*6.5=97。
    assert_eq!(d.trades[0].exit_time, 5);
    assert!(
        (d.trades[0].exit_price - 97.0).abs() < 1e-12,
        "exit={}",
        d.trades[0].exit_price
    );
    assert_eq!(d.trades[0].exit_reason, "trailing_stop");
    assert_eq!(d.summary.trailing_stop_exits, 1);
}

#[test]
fn atr_trailing_stop_never_loosens_when_atr_expands() {
    let open = [100.0; 7];
    let high = [101.0, 101.0, 101.0, 101.0, 110.0, 111.0, 105.0];
    let low = [99.0, 99.0, 99.0, 99.0, 99.0, 100.0, 96.0];
    let close = [100.0, 100.0, 100.0, 100.0, 105.0, 105.0, 100.0];
    let signals = [
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        None,
        StopConfig {
            atr: Some(AtrStopConfig {
                period: 2,
                mult: 2.0,
                trailing: true,
            }),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].entry_time, 4);
    // bar4 后 trail=97; bar5 ATR 放大到 8.75, 裸公式会退到 93.5。
    // 追踪止损必须保持 97, 于是 bar6 low=96 仍按 97 触发。
    assert_eq!(d.trades[0].exit_time, 6);
    assert!(
        (d.trades[0].exit_price - 97.0).abs() < 1e-12,
        "exit={}",
        d.trades[0].exit_price
    );
    assert_eq!(d.trades[0].exit_reason, "trailing_stop");
}

#[test]
fn breakeven_stop_moves_to_entry_after_trigger() {
    let open = [100.0, 100.0, 101.0, 101.0];
    let high = [101.0, 104.0, 101.0, 101.0];
    let low = [99.0, 99.0, 99.0, 99.0];
    let close = [100.0, 103.0, 100.0, 100.0];
    let signals = [Signal::Buy, Signal::Hold, Signal::Hold, Signal::Hold];
    let d = run_stops(
        &open,
        &high,
        &low,
        &close,
        &signals,
        None,
        StopConfig {
            breakeven: Some(BreakevenConfig {
                trigger_pct: 2.0,
                offset_pct: 0.0,
            }),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    assert_eq!(d.trades[0].entry_time, 1);
    assert_eq!(d.trades[0].exit_time, 2);
    assert!((d.trades[0].exit_price - 100.0).abs() < 1e-12);
    assert_eq!(d.trades[0].exit_reason, "trailing_stop");
    assert_eq!(d.summary.trailing_stop_exits, 1);
}

#[test]
fn stop_loss_long_triggers_intrabar_and_fills_at_stop() {
    let open = [100.0, 100.0, 100.0, 98.0, 98.0];
    let high = [101.0, 101.0, 100.0, 99.0, 99.0];
    let low = [99.0, 99.0, 97.0, 97.0, 97.0];
    let close = [100.0, 100.0, 98.0, 98.0, 98.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert_eq!(t.side, Side::Long);
    assert!((t.entry_price - 100.0).abs() < 1e-12);
    assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
    assert_eq!(t.exit_reason, "stop_loss");
    assert_eq!(d.summary.stop_loss_exits, 1);
    assert_eq!(d.summary.trailing_stop_exits, 0);
    assert!((d.summary.final_equity.to_f64() - 980.0).abs() < 1e-9);
}

#[test]
fn stop_loss_short_triggers_intrabar_and_fills_at_stop() {
    let open = [100.0, 100.0, 100.0, 102.0, 102.0];
    let high = [101.0, 101.0, 103.0, 103.0, 103.0];
    let low = [99.0, 99.0, 99.0, 101.0, 101.0];
    let close = [100.0, 100.0, 102.0, 102.0, 102.0];
    let signals = [
        Signal::Sell,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert_eq!(t.side, Side::Short);
    assert!((t.entry_price - 100.0).abs() < 1e-12);
    assert!(
        (t.exit_price - 102.0).abs() < 1e-12,
        "exit={}",
        t.exit_price
    );
    assert_eq!(t.exit_reason, "stop_loss");
    assert_eq!(d.summary.stop_loss_exits, 1);
    assert!((d.summary.final_equity.to_f64() - 980.0).abs() < 1e-9);
}

#[test]
fn stop_loss_gap_fills_at_open_not_stop_price() {
    let open = [100.0, 100.0, 95.0, 95.0, 95.0];
    let high = [101.0, 101.0, 96.0, 96.0, 96.0];
    let low = [99.0, 99.0, 94.0, 94.0, 94.0];
    let close = [100.0, 100.0, 95.0, 95.0, 95.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert!(
        (t.exit_price - 95.0).abs() < 1e-12,
        "跳空必须按 open 成交: {}",
        t.exit_price
    );
    assert_eq!(t.exit_reason, "stop_loss");
    assert!((d.summary.final_equity.to_f64() - 950.0).abs() < 1e-9);
}

#[test]
fn stop_exit_still_applies_slippage_and_fees() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 100.0, 100.0, 100.0];
    let low = [99.0, 99.0, 97.0, 97.0, 97.0];
    let close = [100.0, 100.0, 98.0, 98.0, 98.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let exec = ExecConfig::try_from_bps_fill(1, 10.0, 5.0, false).expect("费率/滑点合法");
    let d = simulate_detailed_ohlc(
        &[0, 1, 2, 3, 4],
        &high,
        &low,
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(1_000),
        252.0,
        &exec,
        &StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
    )
    .expect("合法回测必须成功");
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    // 入场 100×(1+5bp)=100.05, 止损 100.05×0.98=98.049;
    // 平仓再扣 5bp 逆向滑点 ⇒ 97.9999755。
    assert!(
        (t.exit_price - 97.999_975_5).abs() < 1e-9,
        "exit={}",
        t.exit_price
    );
    assert!(t.pnl < crate::money::Amount::ZERO, "止损必须是亏损");
    assert!(
        d.summary.final_equity.to_f64() < 980.0,
        "费用/滑点必须计入止损路径: {}",
        d.summary.final_equity.to_f64()
    );
    assert_eq!(d.summary.stop_loss_exits, 1);
    assert!((d.summary.fee_bps - 10.0).abs() < 1e-12);
    assert!((d.summary.slippage_bps - 5.0).abs() < 1e-12);
}

#[test]
fn trailing_stop_uses_prior_bar_extreme() {
    let open = [100.0, 100.0, 108.0, 108.0, 108.0];
    let high = [101.0, 110.0, 109.0, 109.0, 109.0];
    let low = [99.0, 100.0, 104.0, 104.0, 104.0];
    let close = [100.0, 108.0, 105.0, 105.0, 105.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: None,
            trailing_stop_pct: Some(5.0),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    // bar1 收盘后 extreme=110, 追踪止损=110×0.95=104.5; bar2 盘中触及。
    assert!(
        (t.exit_price - 104.5).abs() < 1e-12,
        "exit={}",
        t.exit_price
    );
    assert_eq!(t.exit_reason, "trailing_stop");
    assert_eq!(d.summary.trailing_stop_exits, 1);
    assert_eq!(d.summary.stop_loss_exits, 0);
    assert!((d.summary.final_equity.to_f64() - 1_045.0).abs() < 1e-9);
}

#[test]
fn trailing_stop_short_ratchets_down_from_lowest_low() {
    let open = [100.0, 100.0, 99.0, 99.0, 99.0];
    let high = [101.0, 101.0, 100.0, 100.0, 100.0];
    let low = [99.0, 95.0, 98.0, 98.0, 98.0];
    let close = [100.0, 99.0, 99.5, 99.5, 99.5];
    let signals = [
        Signal::Sell,
        Signal::Hold,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: None,
            trailing_stop_pct: Some(5.0),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert_eq!(t.side, Side::Short);
    // bar1 收盘后 extreme=95, 空头追踪止损=95×1.05=99.75; bar2 盘中触及。
    assert!(
        (t.exit_price - 99.75).abs() < 1e-12,
        "exit={}",
        t.exit_price
    );
    assert_eq!(t.exit_reason, "trailing_stop");
    assert_eq!(d.summary.trailing_stop_exits, 1);
    assert_eq!(d.summary.stop_loss_exits, 0);
    assert!((d.summary.final_equity.to_f64() - 1_002.5).abs() < 1e-9);
}

#[test]
fn fixed_stop_wins_when_it_is_tighter_than_initial_trailing_stop() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 110.0, 100.0, 100.0, 100.0];
    let low = [99.0, 97.5, 99.0, 99.0, 99.0];
    let close = [100.0, 100.0, 100.0, 100.0, 100.0];
    let signals = [
        Signal::Buy,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: Some(5.0),
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    // 入场当根: 固定止损 98 比追踪止损 95 更紧; 即使 high=110, 追踪止损
    // 也只能在**收盘后**更新, 不得回溯到当根。
    assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
    assert_eq!(t.exit_reason, "stop_loss");
    assert_eq!(d.summary.stop_loss_exits, 1);
    assert_eq!(d.summary.trailing_stop_exits, 0);
}

#[test]
fn stop_checks_before_close_fill_signal() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 101.0];
    let low = [99.0, 99.0, 97.0, 99.0, 99.0];
    let close = [100.0, 100.0, 99.0, 99.0, 99.0];
    let signals = [
        Signal::Buy,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        true,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    // bar1 收盘入场; bar2 盘中 stop 先于 bar1 发出的收盘平仓信号。
    assert!((t.exit_price - 98.0).abs() < 1e-12, "exit={}", t.exit_price);
    assert_eq!(t.exit_reason, "stop_loss");
}

#[test]
fn entry_at_close_does_not_trigger_stop_on_entry_bar() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 101.0];
    // bar1 的 low=90 发生在收盘入场之前, 不得回溯触发 98 的止损。
    let low = [99.0, 90.0, 99.0, 99.0, 99.0];
    let close = [100.0, 100.0, 100.0, 100.0, 100.0];
    let signals = [
        Signal::Buy,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        true,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert_eq!(t.exit_reason, "signal", "收盘入场当根不得触发止损");
    assert!((t.exit_price - 100.0).abs() < 1e-12);
}

#[test]
fn entry_at_open_can_trigger_stop_on_same_bar() {
    let open = [100.0, 100.0, 100.0, 100.0, 100.0];
    let high = [101.0, 101.0, 101.0, 101.0, 101.0];
    let low = [99.0, 97.0, 99.0, 99.0, 99.0];
    let close = [100.0, 100.0, 100.0, 100.0, 100.0];
    let signals = [
        Signal::Buy,
        Signal::Close,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1);
    let t = &d.trades[0];
    assert_eq!(t.exit_reason, "stop_loss");
    assert!((t.exit_price - 98.0).abs() < 1e-12);
    assert_eq!(t.bars_held, 0, "开盘入场当根止损: 持仓 0 bar");
}

#[test]
fn stop_forces_flat_until_a_new_entry_signal() {
    // 止损后目标被重置为 Flat: 后续 Hold 不得把同一方向“接回来”。
    let open = [100.0; 6];
    let high = [101.0; 6];
    let low = [99.0, 99.0, 97.0, 99.0, 99.0, 99.0];
    let close = [100.0, 100.0, 98.0, 99.0, 99.0, 99.0];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 1, "Hold 不应在止损后自动重入");
    assert_eq!(d.trades[0].exit_reason, "stop_loss");

    // 新 Buy 信号仍可在 delay 后重新入场。
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
    ];
    let d = run_stop(
        &open,
        &high,
        &low,
        &close,
        &signals,
        StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
        false,
    );
    assert_eq!(d.trades.len(), 2, "新 Buy 必须能重新入场");
    assert_eq!(d.trades[1].entry_time, 4, "新 Buy 在信号后一根 bar 成交");
    assert_eq!(d.trades[1].exit_reason, "end_of_data");
}

#[test]
fn stop_reset_preserves_delay_two_queue_alignment() {
    // delay=2 + fill_close=true: 止损后队列必须用 Flat 占位而不是 clear,
    // 否则下一根新 Buy 会提前一根 bar 成交。
    let open = [100.0; 8];
    let high = [101.0; 8];
    let low = [99.0, 99.0, 99.0, 97.0, 99.0, 99.0, 99.0, 99.0];
    let close = [100.0; 8];
    let signals = [
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
        Signal::Buy,
        Signal::Hold,
        Signal::Hold,
        Signal::Hold,
    ];
    let exec = ExecConfig::try_from_bps_fill(2, 0.0, 0.0, true).expect("delay=2 / 零费率必须合法");
    let d = simulate_detailed_ohlc(
        &[0, 1, 2, 3, 4, 5, 6, 7],
        &high,
        &low,
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(1_000),
        252.0,
        &exec,
        &StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: None,
            ..StopConfig::default()
        },
    )
    .expect("合法回测必须成功");
    assert_eq!(d.trades.len(), 2);
    assert_eq!(
        d.trades[0].entry_time, 2,
        "信号 bar0 + delay2 在 bar2 收盘成交"
    );
    assert_eq!(d.trades[0].exit_time, 3, "bar3 盘中触发止损");
    assert_eq!(d.trades[0].exit_reason, "stop_loss");
    assert_eq!(
        d.trades[1].entry_time, 6,
        "止损后新 Buy(bar4) 必须等到 bar6 才成交"
    );
    assert_eq!(d.trades[1].exit_reason, "end_of_data");
}

#[test]
fn no_stop_is_bit_identical_to_legacy_simulate() {
    let n = 120;
    let close = make_prices(n);
    let open: Vec<f64> = close.iter().map(|p| p * 0.999).collect();
    let signals: Vec<Signal> = (0..n)
        .map(|i| {
            if i % 17 == 0 {
                Signal::Buy
            } else if i % 29 == 0 {
                Signal::Sell
            } else {
                Signal::Hold
            }
        })
        .collect();
    let exec = ExecConfig::default();
    let a = simulate(
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(10_000),
        252.0,
        &exec,
    )
    .expect("legacy simulate");
    let b = simulate_ohlc(
        &close,
        &close,
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(10_000),
        252.0,
        &exec,
        &StopConfig::default(),
    )
    .expect("no-stop ohlc simulate");
    assert_eq!(a, b, "未配置止损时 high/low 不得改变任何指标");
    assert_eq!(a.stop_loss_exits, 0);
    assert_eq!(a.trailing_stop_exits, 0);
    assert_eq!(a.strategy_stop_exits, 0);

    let da = simulate_detailed(
        &[],
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(10_000),
        252.0,
        &exec,
    )
    .expect("legacy detailed");
    let db = simulate_detailed_ohlc(
        &[],
        &close,
        &close,
        &close,
        &open,
        &signals,
        crate::money::Amount::from_i64(10_000),
        252.0,
        &exec,
        &StopConfig::default(),
    )
    .expect("no-stop ohlc detailed");
    assert_eq!(da.summary, db.summary);
    assert_eq!(da.trades.len(), db.trades.len());
    assert_eq!(da.equity_curve.len(), db.equity_curve.len());
    assert_eq!(da.fixed_curve, db.fixed_curve);
}

/// `exit_reason` 的取值集合必须封闭(落盘层按它做字典序统计)。
#[test]
fn exit_reason_values_are_closed_and_wrong() {
    assert_eq!(ExitReason::Signal.as_str(), "signal");
    assert_eq!(ExitReason::StopLoss.as_str(), "stop_loss");
    assert_eq!(ExitReason::TrailingStop.as_str(), "trailing_stop");
    assert_eq!(ExitReason::StrategyStop.as_str(), "strategy_stop");
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

/// 新 `Ohlcv` / `SimOptions` 入口与旧签名逐位等价。
///
/// 旧签名保留为兼容包装层; 本测试钉住两者在「止损 + ATR 仓位 + 策略止损线」
/// 组合下产出完全相同的指标、交易与曲线。
#[test]
fn ohlcv_api_matches_legacy_signatures() {
    let n = 120;
    let close = make_prices(n);
    let open: Vec<f64> = close.iter().map(|c| c - 0.4).collect();
    let high: Vec<f64> = close.iter().map(|c| c + 1.2).collect();
    let low: Vec<f64> = close.iter().map(|c| c - 1.2).collect();
    let time: Vec<i64> = (0..n as i64).map(|i| i * 60_000).collect();

    let mut signals = vec![Signal::Hold; n];
    signals[4] = Signal::Buy;
    signals[40] = Signal::Close;
    signals[55] = Signal::Sell;
    signals[95] = Signal::ExitShort;

    let exec = ExecConfig::try_from_bps_fill(2, 3.0, 1.5, false).expect("费率/滑点合法");
    let risk = StopConfig {
        stop_loss_pct: Some(2.5),
        trailing_stop_pct: Some(4.0),
        ..StopConfig::default()
    };
    let sizing = SizingConfig::Atr {
        risk_pct: 1.0,
        period: 14,
        mult: 2.0,
        max_position_pct: 50.0,
        fallback_fraction_pct: 10.0,
    };
    let mut long_line = vec![f64::NAN; n];
    long_line[33] = low[33];
    let lines = StopLines {
        long: Some(long_line),
        short: None,
    };
    let cash = crate::money::Amount::from_i64(10_000);
    let opts = SimOptions {
        initial_cash: cash,
        bars_per_year: 252.0,
        exec: &exec,
        risk: &risk,
        sizing: &sizing,
        strategy_stops: Some(&lines),
    };

    let new = simulate_ohlcv(Ohlcv::new(&open, &high, &low, &close), &signals, &opts)
        .expect("新入口必须成功");
    let old = simulate_ohlc_with_stops_and_sizing(
        &high,
        &low,
        &close,
        &open,
        &signals,
        Some(&lines),
        cash,
        252.0,
        &exec,
        &risk,
        &sizing,
    )
    .expect("旧入口必须成功");
    assert_eq!(new, old);

    let new_detailed = simulate_ohlcv_detailed(
        &time,
        Ohlcv::new(&open, &high, &low, &close),
        &signals,
        &opts,
    )
    .expect("新入口必须成功");
    let old_detailed = simulate_detailed_ohlc_with_stops_and_sizing(
        &time,
        &high,
        &low,
        &close,
        &open,
        &signals,
        Some(&lines),
        cash,
        252.0,
        &exec,
        &risk,
        &sizing,
    )
    .expect("旧入口必须成功");
    assert_eq!(new_detailed, old_detailed);
    assert!(
        !new_detailed.trades.is_empty(),
        "等价测试必须真的产生交易, 否则没有覆盖成交路径"
    );

    // close-only 入口: high/low 退化后不得改变任何数值。
    let new_close_only = simulate_ohlcv(
        Ohlcv::close_only(&close, &open),
        &signals,
        &SimOptions::new(cash, 252.0, &exec),
    )
    .expect("新入口必须成功");
    let old_close_only =
        simulate(&close, &open, &signals, cash, 252.0, &exec).expect("旧入口必须成功");
    assert_eq!(new_close_only, old_close_only);
}
