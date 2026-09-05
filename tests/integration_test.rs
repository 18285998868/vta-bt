use vta::config::{PeriodParam, RangeParam};
use vta::engine;
use vta::strategies::Signal;
use vta::strategies::Strategy;
use vta::strategies::ssf::{SsfConfig, SsfStrategy};

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
    });
    assert!(s.param_count() > 0);
    for i in 0..s.param_count() {
        let sig = s.generate_signals(&close, &[], i);
        assert_eq!(sig.len(), 500);
    }
}

#[test]
fn test_engine_all_buy_reinvest_allowed() {
    let close = rising_prices(50);
    let mut sig = vec![Signal::Hold; 50];
    sig[0] = Signal::Buy;
    sig[10] = Signal::Buy;
    let m = engine::simulate(&close, &sig, 1000.0, 365.0);
    assert!(m.total_return_pct > 0.0);
    assert_eq!(m.num_trades, 1);
}

#[test]
fn test_engine_short_profits() {
    let close: Vec<f64> = (0..50).map(|i| 200.0 - i as f64 * 2.0).collect();
    let mut sig = vec![Signal::Hold; 50];
    sig[0] = Signal::Sell;
    sig[25] = Signal::Close;
    let m = engine::simulate(&close, &sig, 1000.0, 365.0);
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
    let m = engine::simulate(&close, &sig, 1000.0, 365.0);
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
    let m = engine::simulate(&close, &sig, 1000.0, 365.0);
    assert_eq!(m.num_trades, 2);
    assert!(m.expectancy.is_finite());
}

#[test]
fn test_engine_cli_override_noop() {
    let close = vec![100.0; 10];
    let sig = vec![Signal::Hold; 10];
    let m = engine::simulate(&close, &sig, 1000.0, 365.0);
    assert_eq!(m.num_trades, 0);
}
