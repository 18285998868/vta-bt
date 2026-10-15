use std::path::Path;

use tempfile::TempDir;
use tradingview_backend::app::routes::AppState;

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

#[test]
fn test_bbrk_end_to_end_markers_equity_summary() {
    let dir = TempDir::new().unwrap();
    write_trend_zip(dir.path());

    let state = AppState::new(dir.path().to_path_buf());
    let result = tradingview_backend::app::strategy::run_backtest(
        &state,
        tradingview_backend::app::data_loader::MarketType::Futures,
        "SYNUSDT",
        "1h",
        "type = \"bbrk\"\nsigma = 2.0",
        10_000.0,
        0.0,
        0.0,
        None,
        None,
    )
    .unwrap();

    assert_eq!(result.strategy, "bbrk");
    assert_eq!(result.count, 40);
    // 1 笔多单：entry + exit 两个 marker，且时间对齐某根 bar。
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.markers.len(), 2);
    assert_eq!(result.markers[0].kind, "entry");
    assert_eq!(result.markers[0].side, "long");
    assert_eq!(result.markers[1].kind, "exit");
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

    let state = AppState::new(dir.path().to_path_buf());
    let err = tradingview_backend::app::strategy::run_backtest(
        &state,
        tradingview_backend::app::data_loader::MarketType::Futures,
        "SYNUSDT",
        "1h",
        "type = \"turtle\"\nentry = 20\nexit = 10",
        10_000.0,
        0.0,
        0.0,
        None,
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        tradingview_backend::app::strategy::StrategyError::User(_)
    ));
}
