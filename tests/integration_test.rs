use std::path::Path;

use polars::prelude::*;
use tempfile::TempDir;

fn write_parquet(
    dir: &Path,
    market: &str,
    pair: &str,
    timeframe: &str,
    rows: &[(i64, f64, f64, f64, f64, f64)],
    suffix: &str,
) {
    let tf_dir = dir
        .join(market)
        .join("um")
        .join("daily")
        .join("klines")
        .join(pair)
        .join(timeframe);
    std::fs::create_dir_all(&tf_dir).unwrap();

    let times: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let opens: Vec<f64> = rows.iter().map(|r| r.1).collect();
    let highs: Vec<f64> = rows.iter().map(|r| r.2).collect();
    let lows: Vec<f64> = rows.iter().map(|r| r.3).collect();
    let closes: Vec<f64> = rows.iter().map(|r| r.4).collect();
    let volumes: Vec<f64> = rows.iter().map(|r| r.5).collect();

    let mut df = df!(
        "open_time" => &times,
        "open" => &opens,
        "high" => &highs,
        "low" => &lows,
        "close" => &closes,
        "volume" => &volumes,
    )
    .unwrap();

    let path = tf_dir.join(format!("{pair}-{timeframe}-{suffix}.parquet"));
    let mut file = std::fs::File::create(path).unwrap();
    ParquetWriter::new(&mut file).finish(&mut df).unwrap();
}

#[test]
fn test_full_pipeline_futures() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().join("futures");
    std::fs::create_dir_all(&data_dir).unwrap();

    write_parquet(
        dir.path(),
        "futures",
        "BTCUSDT",
        "1h",
        &[
            (1_000_000, 100.0, 105.0, 95.0, 102.0, 10.0),
            (2_000_000, 102.0, 107.0, 97.0, 104.0, 20.0),
        ],
        "2024-01-01",
    );
    write_parquet(
        dir.path(),
        "futures",
        "BTCUSDT",
        "1h",
        &[
            (3_000_000, 104.0, 109.0, 99.0, 106.0, 30.0),
            (4_000_000, 106.0, 111.0, 101.0, 108.0, 40.0),
        ],
        "2024-01-02",
    );
    write_parquet(
        dir.path(),
        "futures",
        "ETHUSDT",
        "1h",
        &[(1_000_000, 10.0, 11.0, 9.0, 10.5, 100.0)],
        "2024-01-01",
    );

    let loader = tradingview_backend::data_loader::DataLoader::new(dir.path().to_path_buf());

    let pairs = loader
        .list_pairs(tradingview_backend::data_loader::MarketType::Futures)
        .unwrap();
    assert_eq!(pairs, vec!["BTCUSDT", "ETHUSDT"]);

    let tfs = loader
        .list_timeframes(
            tradingview_backend::data_loader::MarketType::Futures,
            "BTCUSDT",
        )
        .unwrap();
    assert_eq!(tfs, vec!["1h"]);

    let klines = loader
        .load_klines_filtered(
            tradingview_backend::data_loader::MarketType::Futures,
            "BTCUSDT",
            "1h",
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(klines.len(), 4);

    let klines_eth = loader
        .load_klines_filtered(
            tradingview_backend::data_loader::MarketType::Futures,
            "ETHUSDT",
            "1h",
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(klines_eth.len(), 1);
}

#[test]
fn test_klines_dedup_across_files() {
    let dir = TempDir::new().unwrap();
    let rows = &[(1_000_000, 100.0, 105.0, 95.0, 102.0, 10.0)];
    write_parquet(dir.path(), "futures", "BTCUSDT", "1h", rows, "a");
    write_parquet(dir.path(), "futures", "BTCUSDT", "1h", rows, "b");

    let loader = tradingview_backend::data_loader::DataLoader::new(dir.path().to_path_buf());
    let klines = loader
        .load_klines_filtered(
            tradingview_backend::data_loader::MarketType::Futures,
            "BTCUSDT",
            "1h",
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(klines.len(), 1);
}

#[test]
fn test_before_after_limit_combined() {
    let dir = TempDir::new().unwrap();
    write_parquet(
        dir.path(),
        "futures",
        "BTCUSDT",
        "1h",
        &[
            (1_000_000, 100.0, 105.0, 95.0, 102.0, 10.0),
            (2_000_000, 102.0, 107.0, 97.0, 104.0, 20.0),
            (3_000_000, 104.0, 109.0, 99.0, 106.0, 30.0),
        ],
        "2024-01-01",
    );

    let loader = tradingview_backend::data_loader::DataLoader::new(dir.path().to_path_buf());
    let klines = loader
        .load_klines_filtered(
            tradingview_backend::data_loader::MarketType::Futures,
            "BTCUSDT",
            "1h",
            Some(3_500_000),
            Some(500_000),
            Some(1),
        )
        .unwrap();
    assert_eq!(klines.len(), 1);
    // limit=1 takes the last element after filtering: 3_000_000
    assert_eq!(klines[0].time, 3_000_000);
}
