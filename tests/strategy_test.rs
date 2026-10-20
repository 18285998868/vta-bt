use std::path::Path;

use tempfile::TempDir;
use tradingview_backend::app::data_loader::MarketType;
use tradingview_backend::app::routes::AppState;
use tradingview_backend::app::strategy::{BacktestParams, MarkerKind, StrategyError, TradeSide};

/// 20 根 100 横盘 + 20 根上行（110→200）：BBRK 应在突破处开多并持有到末尾。
fn write_trend_zip(dir: &Path) {
    let tf_dir = dir
        .join("binance")
        .join("data")
        .join("um")
        .join("daily")
        .join("klines")
        .join("SYNUSDT")
        .join("1h");
    std::fs::create_dir_all(&tf_dir).unwrap();

    let mut csv = String::new();
    for i in 0..40i64 {
        let c = if i < 20 {
            100.0
        } else {
            100.0 + (i - 19) as f64 * 5.0
        };
        let t = 1_700_000_000_000 + i * 3_600_000;
        csv.push_str(&format!(
            "{t},{c},{},{},{c},10.0,0,0,0,0,0,0\n",
            c + 1.0,
            c - 1.0
        ));
    }
    let zip_path = tf_dir.join("SYNUSDT-1h-2024-01-01.zip");
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file::<&str, ()>(
        "SYNUSDT-1h-2024-01-01.csv",
        zip::write::FileOptions::default(),
    )
    .unwrap();
    use std::io::Write as _;
    zip.write_all(csv.as_bytes()).unwrap();
    zip.finish().unwrap();
}

fn params(strategy_toml: &str) -> BacktestParams<'_> {
    BacktestParams {
        market_type: MarketType::Futures,
        pair: "SYNUSDT",
        timeframe: "1h",
        strategy_toml,
        initial_balance: 10_000.0,
        fee_bps: 0.0,
        slippage_bps: 0.0,
        before: None,
        after: None,
    }
}

#[test]
fn test_bbrk_end_to_end_markers_equity_summary() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());
    let result = tradingview_backend::app::strategy::run_backtest(
        &state,
        &params("type = \"bbrk\"\nsigma = 2.0"),
    )
    .unwrap();

    assert_eq!(result.strategy, "bbrk");
    assert_eq!(result.count, 40);
    // 1 笔多单：entry + exit 两个 marker，且时间对齐某根 bar。
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.markers.len(), 2);
    assert_eq!(result.markers[0].kind, MarkerKind::Entry);
    assert_eq!(result.markers[0].side, TradeSide::Long);
    assert_eq!(result.markers[1].kind, MarkerKind::Exit);
    assert!(result.markers[0].time < result.markers[1].time);
    // 资金曲线与 bar 对齐。
    assert_eq!(result.equity.len(), 40);
    assert!(result.summary.final_equity > 10_000.0);
    assert_eq!(result.summary.num_trades, 1);
}

#[test]
fn test_blocked_strategy_returns_user_error() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());
    let err = tradingview_backend::app::strategy::run_backtest(
        &state,
        &params("type = \"turtle\"\nentry = 20\nexit = 10"),
    )
    .unwrap_err();
    assert!(matches!(err, StrategyError::User(_)));
}

#[test]
fn test_rejects_invalid_balance_and_fees() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());

    let mut p = params("type = \"bbrk\"\nsigma = 2.0");
    p.initial_balance = 0.0;
    assert!(matches!(
        tradingview_backend::app::strategy::run_backtest(&state, &p).unwrap_err(),
        StrategyError::User(_)
    ));

    let mut p = params("type = \"bbrk\"\nsigma = 2.0");
    p.fee_bps = -1.0;
    assert!(matches!(
        tradingview_backend::app::strategy::run_backtest(&state, &p).unwrap_err(),
        StrategyError::User(_)
    ));
}

#[test]
fn test_rejects_out_of_range_sigma() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());
    let err = tradingview_backend::app::strategy::run_backtest(
        &state,
        &params("type = \"bbrk\"\nsigma = 100.0"),
    )
    .unwrap_err();
    assert!(matches!(err, StrategyError::User(_)));
}

#[test]
fn test_time_window_filters_bars() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());
    let mut p = params("type = \"bbrk\"\nsigma = 2.0");
    p.after = Some(1_700_000_000_000 + 10 * 3_600_000);
    let result = tradingview_backend::app::strategy::run_backtest(&state, &p).unwrap();
    // after 为开区间 ⇒ 起点那根被排除 ⇒ 剩 29 根。
    assert_eq!(result.count, 29);
}

#[test]
fn test_missing_pair_returns_not_found() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path());
    let mut p = params("type = \"bbrk\"\nsigma = 2.0");
    p.pair = "NOPEUSDT";
    let err = tradingview_backend::app::strategy::run_backtest(&state, &p).unwrap_err();
    assert!(matches!(err, StrategyError::NotFound(_)));
}
