use std::path::Path;

use tempfile::TempDir;
use tradingview_backend::app::data_loader::{DataLoader, MarketType};

/// 2024-01-01T00:00:00Z；fixture 行必须落在文件名对应的 UTC 日期内。
const BASE_MS: i64 = 1_704_067_200_000;
const HOUR_MS: i64 = 3_600_000;

fn write_zip(
    dir: &Path,
    pair: &str,
    timeframe: &str,
    rows: &[(i64, f64, f64, f64, f64, f64)],
    suffix: &str,
) {
    let tf_dir = dir
        .join("binance")
        .join("data")
        .join("futures")
        .join("um")
        .join("daily")
        .join("klines")
        .join(pair)
        .join(timeframe);
    std::fs::create_dir_all(&tf_dir).unwrap();

    let mut csv = String::new();
    for r in rows {
        // fixture 都是 1h bar：close_time 必须是 open + 1h - 1ms。
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},0,0,0,0,0\n",
            r.0,
            r.1,
            r.2,
            r.3,
            r.4,
            r.5,
            r.0 + HOUR_MS - 1
        ));
    }
    let name = format!("{pair}-{timeframe}-{suffix}.zip");
    let zip_path = tf_dir.join(&name);
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file::<&str, ()>(
        &name.replace(".zip", ".csv"),
        zip::write::FileOptions::default(),
    )
    .unwrap();
    use std::io::Write as _;
    zip.write_all(csv.as_bytes()).unwrap();
    zip.finish().unwrap();
}

#[test]
fn test_full_pipeline_futures() {
    let dir = TempDir::new().unwrap();

    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (BASE_MS, 100.0, 105.0, 95.0, 102.0, 10.0),
            (BASE_MS + HOUR_MS, 102.0, 107.0, 97.0, 104.0, 20.0),
        ],
        "2024-01-01",
    );
    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (BASE_MS + 24 * HOUR_MS, 104.0, 109.0, 99.0, 106.0, 30.0),
            (BASE_MS + 25 * HOUR_MS, 106.0, 111.0, 101.0, 108.0, 40.0),
        ],
        "2024-01-02",
    );
    write_zip(
        dir.path(),
        "ETHUSDT",
        "1h",
        &[(BASE_MS, 10.0, 11.0, 9.0, 10.5, 100.0)],
        "2024-01-01",
    );

    let loader = DataLoader::new(dir.path());

    let pairs = loader.list_pairs(MarketType::Futures).unwrap();
    assert_eq!(pairs, vec!["BTCUSDT", "ETHUSDT"]);

    let tfs = loader
        .list_timeframes(MarketType::Futures, "BTCUSDT")
        .unwrap();
    assert_eq!(tfs, vec!["1h"]);

    let klines = loader
        .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
        .unwrap();
    assert_eq!(klines.len(), 4);

    let klines_eth = loader
        .load_klines_filtered(MarketType::Futures, "ETHUSDT", "1h", None, None, None)
        .unwrap();
    assert_eq!(klines_eth.len(), 1);
}

#[test]
fn test_duplicate_rows_across_files_are_rejected() {
    // 新版 binance reader 在加载阶段校验时间戳必须落在文件名对应的 UTC 日期内，
    // 因此把同一条 bar 写进两个不同日期的文件会直接报错，而不是静默去重。
    let dir = TempDir::new().unwrap();
    let rows = &[(BASE_MS, 100.0, 105.0, 95.0, 102.0, 10.0)];
    write_zip(dir.path(), "BTCUSDT", "1h", rows, "2024-01-01");
    write_zip(dir.path(), "BTCUSDT", "1h", rows, "2024-01-02");

    let loader = DataLoader::new(dir.path());
    let result =
        loader.load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None);
    assert!(result.is_err());
}

#[test]
fn test_before_after_limit_combined() {
    let dir = TempDir::new().unwrap();
    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (BASE_MS, 100.0, 105.0, 95.0, 102.0, 10.0),
            (BASE_MS + HOUR_MS, 102.0, 107.0, 97.0, 104.0, 20.0),
            (BASE_MS + 2 * HOUR_MS, 104.0, 109.0, 99.0, 106.0, 30.0),
        ],
        "2024-01-01",
    );

    let loader = DataLoader::new(dir.path());
    let klines = loader
        .load_klines_filtered(
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            Some(BASE_MS + 2 * HOUR_MS + HOUR_MS / 2),
            Some(BASE_MS - HOUR_MS / 2),
            Some(1),
        )
        .unwrap();
    assert_eq!(klines.len(), 1);
    // limit=1 takes the last element after filtering: the 02:00 bar.
    assert_eq!(klines[0].time, BASE_MS + 2 * HOUR_MS);
}

#[test]
fn test_limit_takes_last_bars() {
    let dir = TempDir::new().unwrap();
    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (BASE_MS, 100.0, 105.0, 95.0, 102.0, 10.0),
            (BASE_MS + HOUR_MS, 102.0, 107.0, 97.0, 104.0, 20.0),
            (BASE_MS + 2 * HOUR_MS, 104.0, 109.0, 99.0, 106.0, 30.0),
        ],
        "2024-01-01",
    );

    let loader = DataLoader::new(dir.path());
    let klines = loader
        .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, Some(2))
        .unwrap();
    assert_eq!(klines.len(), 2);
    assert_eq!(klines[0].time, BASE_MS + HOUR_MS);
    assert_eq!(klines[1].time, BASE_MS + 2 * HOUR_MS);
}

#[test]
fn test_columns_api_matches_klines_api() {
    let dir = TempDir::new().unwrap();
    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (BASE_MS, 100.0, 105.0, 95.0, 102.0, 10.0),
            (BASE_MS + HOUR_MS, 102.0, 107.0, 97.0, 104.0, 20.0),
            (BASE_MS + 2 * HOUR_MS, 104.0, 109.0, 99.0, 106.0, 30.0),
        ],
        "2024-01-01",
    );

    let loader = DataLoader::new(dir.path());
    let klines = loader
        .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
        .unwrap();
    let cols = loader
        .load_columns_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None)
        .unwrap();

    assert_eq!(cols.len(), klines.len());
    for (i, k) in klines.iter().enumerate() {
        assert_eq!(cols.time[i], k.time);
        assert_eq!(cols.open[i], k.open);
        assert_eq!(cols.high[i], k.high);
        assert_eq!(cols.low[i], k.low);
        assert_eq!(cols.close[i], k.close);
        assert_eq!(cols.volume[i], k.volume);
    }
}
