use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::data_loader::{DataLoader, MarketType};

/// 应用程序共享状态
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

/// API 响应包装
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

/// /api/market-types 返回值
#[derive(Serialize)]
struct MarketTypesResponse {
    types: Vec<String>,
}

/// /api/pairs 查询参数
#[derive(Debug, Deserialize)]
pub struct PairsQuery {
    #[serde(default)]
    pub market_type: Option<String>,
}

/// /api/timeframes 查询参数
#[derive(Debug, Deserialize)]
pub struct TimeframesQuery {
    pub market_type: Option<String>,
    pub pair: String,
}

/// /api/klines 查询参数
#[derive(Debug, Deserialize)]
pub struct KlinesQuery {
    pub market_type: Option<String>,
    pub pair: String,
    pub timeframe: String,
    pub limit: Option<usize>,
    /// Return only klines before this timestamp (ms).
    pub before: Option<i64>,
    /// Return only klines after this timestamp (ms).
    pub after: Option<i64>,
}

fn parse_market_type(market_type: &Option<String>) -> MarketType {
    market_type
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

/// GET /api/health
async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(serde_json::json!({
            "status": "ok",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))),
    )
}

/// GET /api/market-types
async fn market_types_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(MarketTypesResponse {
            types: vec!["futures".to_string(), "spot".to_string()],
        })),
    )
}

fn handle_error(
    status: StatusCode,
    e: impl std::fmt::Display,
) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    tracing::error!(error = %e, "request failed");
    (status, Json(ApiResponse::err("internal error")))
}

/// GET /api/pairs
async fn pairs_handler(
    State(state): State<AppState>,
    Query(query): Query<PairsQuery>,
) -> impl IntoResponse {
    let market_type = parse_market_type(&query.market_type);

    match state.data_loader.list_pairs(market_type) {
        Ok(pairs) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pairs": pairs
            }))),
        ),
        Err(e) => handle_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// GET /api/timeframes
async fn timeframes_handler(
    State(state): State<AppState>,
    Query(query): Query<TimeframesQuery>,
) -> impl IntoResponse {
    let market_type = parse_market_type(&query.market_type);

    match state.data_loader.list_timeframes(market_type, &query.pair) {
        Ok(timeframes) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframes": timeframes
            }))),
        ),
        Err(e) => handle_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// GET /api/klines
async fn klines_handler(
    State(state): State<AppState>,
    Query(query): Query<KlinesQuery>,
) -> impl IntoResponse {
    let market_type = parse_market_type(&query.market_type);

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
        Err(e) => handle_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// 创建所有路由。
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

    #[test]
    fn test_parse_market_type_default() {
        assert_eq!(parse_market_type(&None), MarketType::Futures);
        assert_eq!(
            parse_market_type(&Some("futures".to_string())),
            MarketType::Futures
        );
        assert_eq!(
            parse_market_type(&Some("spot".to_string())),
            MarketType::Spot
        );
        assert_eq!(
            parse_market_type(&Some("invalid".to_string())),
            MarketType::Futures
        );
    }
}
