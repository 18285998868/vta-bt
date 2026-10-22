use std::path::Path;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use tradingview_backend::app::AppState;
use tradingview_backend::app::routes::create_router;
use tradingview_backend::app::state::StateStore;

fn write_zip(dir: &Path, pair: &str, timeframe: &str, rows: &[(i64, f64, f64, f64, f64, f64)]) {
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
        csv.push_str(&format!(
            "{},{},{},{},{},{},0,0,0,0,0,0\n",
            r.0, r.1, r.2, r.3, r.4, r.5
        ));
    }
    let name = format!("{pair}-{timeframe}-2023-11-14.zip");
    let file = std::fs::File::create(tf_dir.join(&name)).unwrap();
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

fn app(dir: &Path) -> axum::Router {
    let store = Arc::new(StateStore::open_in_memory().unwrap());
    create_router(AppState::new(dir, store))
}

async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn udf_fixture_endpoints() {
    let dir = TempDir::new().unwrap();
    write_zip(
        dir.path(),
        "BTCUSDT",
        "1h",
        &[
            (1_700_000_000_000, 100.0, 105.0, 95.0, 102.0, 10.0),
            (1_700_003_600_000, 102.0, 107.0, 97.0, 104.0, 20.0),
            (1_700_007_200_000, 104.0, 108.0, 100.0, 101.0, 30.0),
        ],
    );
    let app = app(dir.path());

    let (status, config) = get_json(&app, "/udf/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["supports_search"], true);
    assert!(config["supported_resolutions"].as_array().unwrap().len() >= 3);

    let (status, time) = get_json(&app, "/udf/time").await;
    assert_eq!(status, StatusCode::OK);
    assert!(time["time"].as_i64().unwrap() > 1_700_000_000);

    let (status, search) = get_json(&app, "/udf/search?query=BTC&limit=5").await;
    assert_eq!(status, StatusCode::OK);
    let results = search.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["symbol"], "BINANCE:BTCUSDT");
    assert_eq!(results[0]["type"], "crypto");

    let (status, symbol) = get_json(&app, "/udf/symbols?symbol=BINANCE:BTCUSDT").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(symbol["name"], "BTCUSDT");
    assert_eq!(symbol["exchange"], "BINANCE");
    assert_eq!(symbol["has_intraday"], true);
    assert!(
        symbol["supported_resolutions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "60")
    );

    let (status, history) = get_json(
        &app,
        "/udf/history?symbol=BINANCE:BTCUSDT&resolution=60&from=1700000000&to=1700007200",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["s"], "ok");
    assert_eq!(
        history["t"].as_array().unwrap(),
        &vec![
            serde_json::json!(1_700_000_000_i64),
            serde_json::json!(1_700_003_600_i64),
            serde_json::json!(1_700_007_200_i64),
        ]
    );
    assert_eq!(history["o"].as_array().unwrap().len(), 3);
    assert_eq!(history["c"].as_array().unwrap()[2], 101.0);

    let (status, no_data) = get_json(
        &app,
        "/udf/history?symbol=BINANCE:NOPE&resolution=60&from=0&to=0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(no_data["s"], "no_data");

    let (status, bad_res) = get_json(
        &app,
        "/udf/history?symbol=BINANCE:BTCUSDT&resolution=2D&from=0&to=0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bad_res["s"], "error");
}
