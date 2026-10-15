use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use super::data_loader::{DataError, DataLoader, MarketType, ParseMarketTypeError};
use super::strategy::{StrategyError, StrategyResult};
use crate::utils::{valid_pair, valid_timeframe};

#[derive(Clone)]
pub struct AppState {
    pub(crate) data_loader: Arc<DataLoader>,
    pub(crate) strategy_cache: moka::sync::Cache<String, Arc<StrategyResult>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_loader: Arc::new(DataLoader::new(data_dir)),
            strategy_cache: moka::sync::Cache::builder()
                .max_capacity(100)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
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

#[derive(Debug, Deserialize)]
pub struct StrategyQuery {
    pub market_type: Option<String>,
    pub pair: String,
    pub timeframe: String,
    pub strategy_toml: String,
    pub initial_balance: Option<f64>,
    pub fee_bps: Option<f64>,
    pub slippage_bps: Option<f64>,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

fn parse_market_type(market_type: &Option<String>) -> Result<MarketType, ParseMarketTypeError> {
    market_type
        .as_ref()
        .map_or(Ok(MarketType::default()), |s| s.parse())
}

fn require_market_type(
    market_type: &Option<String>,
) -> Result<MarketType, (StatusCode, Json<ApiResponse<serde_json::Value>>)> {
    parse_market_type(market_type).map_err(|e| bad_request(e.to_string()))
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
    let market_type = match require_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(resp) => return resp,
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
    let market_type = match require_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(resp) => return resp,
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
    let market_type = match require_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(resp) => return resp,
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

fn strategy_error(e: StrategyError) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    match e {
        StrategyError::User(msg) => bad_request(msg),
        StrategyError::NotFound(msg) => (StatusCode::NOT_FOUND, Json(ApiResponse::err(msg))),
        StrategyError::Internal(_) => {
            tracing::error!(error = ?e, "strategy failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err("internal error")),
            )
        }
    }
}

async fn strategies_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(serde_json::json!({
            "strategies": super::strategy::list_strategies()
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
        }))),
    )
}

async fn strategy_handler(
    State(state): State<AppState>,
    Query(query): Query<StrategyQuery>,
) -> impl IntoResponse {
    let market_type = match require_market_type(&query.market_type) {
        Ok(mt) => mt,
        Err(resp) => return resp,
    };

    if !valid_pair(&query.pair) {
        return bad_request("invalid pair");
    }
    if !valid_timeframe(&query.timeframe) {
        return bad_request("invalid timeframe");
    }

    match super::strategy::run_backtest(
        &state,
        market_type,
        &query.pair,
        &query.timeframe,
        &query.strategy_toml,
        query.initial_balance.unwrap_or(10_000.0),
        query.fee_bps.unwrap_or(0.0),
        query.slippage_bps.unwrap_or(0.0),
        query.before,
        query.after,
    ) {
        Ok(result) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframe": query.timeframe,
                "strategy": result.strategy,
                "count": result.count,
                "markers": result.markers,
                "trades": result.trades,
                "equity": result.equity,
                "overlays": result.overlays,
                "summary": result.summary
            }))),
        ),
        Err(e) => strategy_error(e),
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
        .route("/api/strategies", axum::routing::get(strategies_handler))
        .route("/api/strategy", axum::routing::get(strategy_handler))
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
    async fn test_strategies_lists_allowlist() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/strategies")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_strategy_rejects_bad_toml() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/strategy?market_type=futures&pair=BTCUSDT&timeframe=1h&strategy_toml=type%20%3D%20%22nope%22")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_strategy_rejects_traversal_pair() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/strategy?market_type=futures&pair=..%2F..&timeframe=1h&strategy_toml=type%20%3D%20%22bbrk%22")
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
