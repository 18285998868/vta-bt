use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::data_loader::{DataError, DataLoader, MarketType, ParseMarketTypeError};

#[derive(Clone)]
pub struct AppState {
    data_loader: Arc<DataLoader>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_loader: Arc::new(DataLoader::new(data_dir)),
        }
    }
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

#[derive(Serialize)]
struct MarketTypesResponse {
    types: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PairsQuery {
    #[serde(default)]
    pub market_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TimeframesQuery {
    pub market_type: Option<String>,
    pub pair: String,
}

#[derive(Debug, Deserialize)]
pub struct KlinesQuery {
    pub market_type: Option<String>,
    pub pair: String,
    pub timeframe: String,
    pub limit: Option<usize>,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

fn parse_market_type(market_type: &Option<String>) -> Result<MarketType, ParseMarketTypeError> {
    market_type
        .as_ref()
        .map_or(Ok(MarketType::default()), |s| s.parse())
}

const PAIR_ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
const TIMEFRAME_ALLOWED: &str = "0123456789abcdefghijklmnopqrstuvwxyz_";

fn is_allowed(s: &str, allowed: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| allowed.contains(b as char))
}

fn valid_pair(s: &str) -> bool {
    is_allowed(s, PAIR_ALLOWED)
}

fn valid_timeframe(s: &str) -> bool {
    is_allowed(s, TIMEFRAME_ALLOWED)
}

async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(serde_json::json!({
            "status": "ok",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))),
    )
}

async fn market_types_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(MarketTypesResponse {
            types: vec!["futures".to_string(), "spot".to_string()],
        })),
    )
}

fn handle_data_error(e: DataError) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    match &e {
        DataError::NotFound(_) => {
            tracing::warn!(error = ?e, "resource not found");
            (StatusCode::NOT_FOUND, Json(ApiResponse::err(e.to_string())))
        }
        _ => {
            tracing::error!(error = ?e, "request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err("internal error")),
            )
        }
    }
}

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    (StatusCode::BAD_REQUEST, Json(ApiResponse::err(msg)))
}

async fn pairs_handler(
    State(state): State<AppState>,
    Query(query): Query<PairsQuery>,
) -> impl IntoResponse {
    let market_type = match parse_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(e) => return bad_request(e.to_string()),
    };

    match state.data_loader.list_pairs(market_type) {
        Ok(pairs) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pairs": pairs
            }))),
        ),
        Err(e) => handle_data_error(e),
    }
}

async fn timeframes_handler(
    State(state): State<AppState>,
    Query(query): Query<TimeframesQuery>,
) -> impl IntoResponse {
    let market_type = match parse_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(e) => return bad_request(e.to_string()),
    };

    if !valid_pair(&query.pair) {
        return bad_request("invalid pair");
    }

    match state.data_loader.list_timeframes(market_type, &query.pair) {
        Ok(timeframes) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframes": timeframes
            }))),
        ),
        Err(e) => handle_data_error(e),
    }
}

async fn klines_handler(
    State(state): State<AppState>,
    Query(query): Query<KlinesQuery>,
) -> impl IntoResponse {
    let market_type = match parse_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(e) => return bad_request(e.to_string()),
    };

    if !valid_pair(&query.pair) {
        return bad_request("invalid pair");
    }
    if !valid_timeframe(&query.timeframe) {
        return bad_request("invalid timeframe");
    }

    match state.data_loader.load_klines_filtered(
        market_type,
        &query.pair,
        &query.timeframe,
        query.before,
        query.after,
        query.limit,
    ) {
        Ok(klines) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframe": query.timeframe,
                "count": klines.len(),
                "klines": klines
            }))),
        ),
        Err(e) => handle_data_error(e),
    }
}

pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/api/health", axum::routing::get(health_handler))
        .route(
            "/api/market-types",
            axum::routing::get(market_types_handler),
        )
        .route("/api/pairs", axum::routing::get(pairs_handler))
        .route("/api/timeframes", axum::routing::get(timeframes_handler))
        .route("/api/klines", axum::routing::get(klines_handler))
        .with_state(app_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn test_parse_market_type_default() {
        assert_eq!(parse_market_type(&None).unwrap(), MarketType::Futures);
        assert_eq!(
            parse_market_type(&Some("futures".to_string())).unwrap(),
            MarketType::Futures
        );
        assert_eq!(
            parse_market_type(&Some("spot".to_string())).unwrap(),
            MarketType::Spot
        );
        assert!(parse_market_type(&Some("invalid".to_string())).is_err());
    }

    #[test]
    fn test_valid_pair_timeframe_pure() {
        assert!(valid_pair("BTCUSDT"));
        assert!(valid_pair("BTC_USDT"));
        assert!(!valid_pair(""));
        assert!(!valid_pair(".."));
        assert!(!valid_pair("../../etc"));
        assert!(!valid_pair("a/b"));
        assert!(!valid_pair("a\\b"));
        assert!(!valid_pair("a b"));
        assert!(!valid_pair("btcusdt"));
        assert!(!valid_pair(".hidden"));

        assert!(valid_timeframe("1h"));
        assert!(valid_timeframe("15m"));
        assert!(valid_timeframe("1_2h"));
        assert!(!valid_timeframe(""));
        assert!(!valid_timeframe(".."));
        assert!(!valid_timeframe("1/h"));
        assert!(!valid_timeframe("1h "));
        assert!(!valid_timeframe("1H"));
    }

    fn test_app() -> Router {
        create_router(AppState::new(PathBuf::new()))
    }

    #[tokio::test]
    async fn test_klines_rejects_traversal_pair() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/klines?market_type=futures&pair=..%2F..%2Fetc&timeframe=1h")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_klines_rejects_absolute_timeframe() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/klines?market_type=futures&pair=BTCUSDT&timeframe=%2Fetc%2Fpasswd")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_timeframes_rejects_invalid_pair() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/timeframes?market_type=futures&pair=..%2F..")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_klines_accepts_valid_input() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/klines?market_type=futures&pair=BTCUSDT&timeframe=1h")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
