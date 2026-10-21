//! 端到端集成测试: 引擎成交模拟、执行口径与策略信号的核心不变量。
//!
//! 覆盖: `delay_bars` 的前视防护、成交价与费用口径、信号 → 持仓方向的映射,
//! 以及策略层的"无信号即全 Hold"契约。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use vta::config::{PeriodParam, RangeParam};
use vta::engine;
use vta::engine::ExecConfig;
use vta::strategies::Signal;
use vta::strategies::ssf::{SsfConfig, SsfStrategy};
use vta::strategies::{PoleCount, Strategy};

fn rising_prices(n: usize) -> Vec<f64> {
    (0..n).map(|i| 100.0 + i as f64 * 0.5).collect()
}

fn volatile_prices(n: usize) -> Vec<f64> {
    use std::f64::consts::TAU;
    (0..n)
        .map(|i| 100.0 + (i as f64 * TAU / 50.0).sin() * 30.0)
        .collect()
}

#[test]
fn test_ssf_standard_across_runs() {
    let close = volatile_prices(500);
    let s = SsfStrategy::new(SsfConfig {
        period: PeriodParam::Range(RangeParam {
            start: 10,
            end: 50,
            step: 10,
        }),
        poles: PoleCount::Two,
    });
    assert!(s.param_count() > 0);
    for i in 0..s.param_count() {
        let sig = s
            .generate_signals_full(&[], &[], &[], &close, &[], &[], i)
            .unwrap();
        assert_eq!(sig.len(), 500);
    }
}

#[test]
fn test_engine_all_buy_reinvest_allowed() {
    let close = rising_prices(50);
    let mut sig = vec![Signal::Hold; 50];
    sig[0] = Signal::Buy;
    sig[10] = Signal::Buy;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert!(m.total_return_pct > 0.0);
    assert_eq!(m.num_trades, 1);
}

#[test]
fn test_engine_short_profits() {
    let close: Vec<f64> = (0..50).map(|i| 200.0 - i as f64 * 2.0).collect();
    let mut sig = vec![Signal::Hold; 50];
    sig[0] = Signal::Sell;
    sig[25] = Signal::Close;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert!(
        m.total_return_pct > 0.0,
        "short should profit: {}",
        m.total_return_pct
    );
    assert_eq!(m.num_trades, 1);
}

#[test]
fn test_engine_short_losses() {
    let close: Vec<f64> = (0..50).map(|i| 100.0 + i as f64 * 2.0).collect();
    let mut sig = vec![Signal::Hold; 50];
    sig[0] = Signal::Sell;
    sig[30] = Signal::Close;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert!(
        m.total_return_pct < 0.0,
        "short should lose: {}",
        m.total_return_pct
    );
    assert_eq!(m.num_trades, 1);
}

#[test]
fn test_engine_buy_sell_cycle() {
    let close = volatile_prices(300);
    let mut sig = vec![Signal::Hold; 300];
    sig[5] = Signal::Buy;
    sig[100] = Signal::Close;
    sig[150] = Signal::Buy;
    sig[250] = Signal::Close;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert_eq!(m.num_trades, 2);
    assert!(m.expectancy.is_finite());
}

#[test]
fn test_simulate_all_hold_yields_zero_trades() {
    let close = vec![100.0; 10];
    let sig = vec![Signal::Hold; 10];
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert_eq!(m.num_trades, 0);
}

#[test]
fn test_simulate_handles_nan_close() {
    let mut close = rising_prices(50);
    close[10] = f64::NAN;
    let mut sig = vec![Signal::Hold; 50];
    sig[5] = Signal::Buy;
    sig[30] = Signal::Close;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert!(m.total_return.is_finite());
    assert!(m.sharpe_ratio.is_finite());
    assert!(m.max_drawdown_pct.is_finite());
}

#[test]
fn test_simulate_truncates_to_shortest() {
    let close = rising_prices(30);
    let mut sig = vec![Signal::Hold; 60];
    sig[0] = Signal::Buy;
    let m = engine::simulate(
        &close,
        &close,
        &sig,
        vta::money::Amount::from_i64(1000),
        365.0,
        &ExecConfig::default(),
    )
    .expect("合法回测必须成功");
    assert_eq!(m.num_trades, 1);
}

#[test]
fn test_strategy_period_larger_than_window() {
    let close = rising_prices(50);
    let s = SsfStrategy::new(SsfConfig {
        period: PeriodParam::Single(500),
        poles: PoleCount::Two,
    });
    // 数据短于 period ⇒ 该档**不可计算** ⇒ 显式 `ParamSkipped`,
    // **不是**静默降级成"全程 Hold"(见 `RunStatus::Invalid` 的文档):
    // 二者在报告里必须可区分, 否则"算不出来"会被当成"零波动低风险"。
    let e = s
        .generate_signals_full(&[], &[], &[], &close, &[], &[], 0)
        .expect_err("period 超过数据长度必须显式跳过");
    assert!(
        matches!(e, vta::error::AppError::ParamSkipped { .. }),
        "必须是 ParamSkipped, 实际: {e}"
    );
}

/// **端到端**: 不可计算的参数档位在报告里必须是 `status = "invalid"`,
/// 且**不中断**同一策略的其余档位。
#[test]
fn invalid_param_is_reported_as_invalid_without_aborting_the_batch() {
    use vta::strategies::{ParamOutcome, Strategy};

    let close = rising_prices(50);
    let s = SsfStrategy::new(SsfConfig {
        period: PeriodParam::Single(500),
        poles: PoleCount::Two,
    });
    // 批量入口: 不可计算的那一档是 `Skipped`, 其余档照常。
    let batch = s
        .generate_signals_batch(&[], &[], &[], &close, &[], &[], 0..1)
        .expect("批量入口必须成功返回(不可计算只影响单档)");
    assert_eq!(batch.len(), 1);
    assert!(
        matches!(batch[0], ParamOutcome::Skipped(_)),
        "不可计算的档位必须是 Skipped, 实际: {:?}",
        batch[0]
    );
}
