//! HTTP 路由：参数校验 → 数据加载 → 统一响应体。

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
use super::strategy::{BacktestParams, StrategyError};
use crate::utils::{valid_pair, valid_timeframe};

/// 单次 `/api/klines` 返回的最大 bar 数（防御超大响应体）。
const MAX_KLINE_LIMIT: usize = 10_000;
/// 单次 `/api/strategy` 允许的策略 TOML 字节数（防御超大解析输入）。
const MAX_STRATEGY_TOML_BYTES: usize = 4_096;

/// 服务共享状态。
#[derive(Clone)]
pub struct AppState {
    pub(crate) data_loader: Arc<DataLoader>,
    pub(crate) strategy_cache: moka::sync::Cache<String, Arc<super::strategy::StrategyResult>>,
}

impl AppState {
    /// 构造服务状态。
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_loader: Arc::new(DataLoader::new(data_dir)),
            strategy_cache: moka::sync::Cache::builder()
                .max_capacity(100)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
        }
    }
}

/// 统一响应体：成功时 `data` 有值，失败时 `error` 有值。
#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T> ApiResponse<T> {
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

/// 错误响应（`data` 恒为 `null`）。
type ErrorResponse = (StatusCode, Json<ApiResponse<serde_json::Value>>);

/// 市场类型列表响应。
#[derive(Serialize)]
struct MarketTypesResponse {
    types: Vec<String>,
}

/// `/api/pairs` 查询参数。
#[derive(Debug, Deserialize)]
pub struct PairsQuery {
    /// 市场类型（缺省为 `futures`）。
    pub market_type: Option<String>,
}

/// `/api/timeframes` 查询参数。
#[derive(Debug, Deserialize)]
pub struct TimeframesQuery {
    /// 市场类型（缺省为 `futures`）。
    pub market_type: Option<String>,
    /// 交易对。
    pub pair: String,
}

/// `/api/klines` 查询参数。
#[derive(Debug, Deserialize)]
pub struct KlinesQuery {
    /// 市场类型（缺省为 `futures`）。
    pub market_type: Option<String>,
    /// 交易对。
    pub pair: String,
    /// 周期。
    pub timeframe: String,
    /// 最多返回的 bar 数（取窗口内最后 N 根）。
    pub limit: Option<usize>,
    /// 时间窗口上界（不含，毫秒时间戳）。
    pub before: Option<i64>,
    /// 时间窗口下界（不含，毫秒时间戳）。
    pub after: Option<i64>,
}

/// `/api/strategy` 查询参数。
#[derive(Debug, Deserialize)]
pub struct StrategyQuery {
    /// 市场类型（缺省为 `futures`）。
    pub market_type: Option<String>,
    /// 交易对。
    pub pair: String,
    /// 周期。
    pub timeframe: String,
    /// 策略 TOML 片段。
    pub strategy_toml: String,
    /// 初始资金（缺省 10000）。
    pub initial_balance: Option<f64>,
    /// 单边手续费（基点，缺省 0）。
    pub fee_bps: Option<f64>,
    /// 单边滑点（基点，缺省 0）。
    pub slippage_bps: Option<f64>,
    /// 时间窗口上界（不含，毫秒时间戳）。
    pub before: Option<i64>,
    /// 时间窗口下界（不含，毫秒时间戳）。
    pub after: Option<i64>,
}

/// 解析市场类型（缺省 `futures`）。
fn parse_market_type(market_type: Option<&str>) -> Result<MarketType, ParseMarketTypeError> {
    market_type.map_or(Ok(MarketType::default()), str::parse)
}

/// 解析市场类型，失败转 400 响应。
fn require_market_type(market_type: Option<&str>) -> Result<MarketType, ErrorResponse> {
    parse_market_type(market_type).map_err(|e| bad_request(e.to_string()))
}

/// 构造 400 响应。
fn bad_request(msg: impl Into<String>) -> ErrorResponse {
    (StatusCode::BAD_REQUEST, Json(ApiResponse::err(msg)))
}

/// 健康检查。
#[tracing::instrument(skip_all)]
async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(serde_json::json!({
            "status": "ok",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))),
    )
}

/// 市场类型列表。
#[tracing::instrument(skip_all)]
async fn market_types_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(MarketTypesResponse {
            types: MarketType::ALL.iter().map(|m| m.to_string()).collect(),
        })),
    )
}

/// 数据错误 → HTTP 响应（500 一律脱敏，不外泄内部细节）。
fn handle_data_error(e: DataError) -> ErrorResponse {
    match &e {
        DataError::NotFound(_) => {
            tracing::warn!(error = %e, "resource not found");
            (StatusCode::NOT_FOUND, Json(ApiResponse::err(e.to_string())))
        }
        _ => {
            tracing::error!(error = %e, "request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err("internal error")),
            )
        }
    }
}

/// `/api/pairs`：列出交易对。
#[tracing::instrument(skip_all)]
async fn pairs_handler(
    State(state): State<AppState>,
    Query(query): Query<PairsQuery>,
) -> impl IntoResponse {
    let market_type = match require_market_type(query.market_type.as_deref()) {
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

/// `/api/timeframes`：列出某交易对下的周期。
#[tracing::instrument(skip_all)]
async fn timeframes_handler(
    State(state): State<AppState>,
    Query(query): Query<TimeframesQuery>,
) -> impl IntoResponse {
    let market_type = match require_market_type(query.market_type.as_deref()) {
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

/// `/api/klines`：取 K 线。
#[tracing::instrument(skip_all, fields(pair = %query.pair, timeframe = %query.timeframe))]
async fn klines_handler(
    State(state): State<AppState>,
    Query(query): Query<KlinesQuery>,
) -> impl IntoResponse {
    let market_type = match require_market_type(query.market_type.as_deref()) {
        Ok(mt) => mt,
        Err(resp) => return resp,
    };

    if !valid_pair(&query.pair) {
        return bad_request("invalid pair");
    }
    if !valid_timeframe(&query.timeframe) {
        return bad_request("invalid timeframe");
    }
    if query.limit.is_some_and(|n| n > MAX_KLINE_LIMIT) {
        return bad_request(format!("limit too large (max {MAX_KLINE_LIMIT})"));
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

/// 策略错误 → HTTP 响应（500 一律脱敏）。
fn strategy_error(e: StrategyError) -> ErrorResponse {
    match &e {
        StrategyError::User(msg) => bad_request(msg.clone()),
        StrategyError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, Json(ApiResponse::err(msg.clone())))
        }
        StrategyError::Internal(_) => {
            tracing::error!(error = %e, "strategy failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err("internal error")),
            )
        }
    }
}

/// `/api/strategies`：策略下拉列表。
#[tracing::instrument(skip_all)]
async fn strategies_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(serde_json::json!({
            "strategies": super::strategy::list_strategies()
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
        }))),
    )
}

/// `/api/strategy`：回测。
#[tracing::instrument(skip_all, fields(pair = %query.pair, timeframe = %query.timeframe))]
async fn strategy_handler(
    State(state): State<AppState>,
    Query(query): Query<StrategyQuery>,
) -> impl IntoResponse {
    let market_type = match require_market_type(query.market_type.as_deref()) {
        Ok(mt) => mt,
        Err(resp) => return resp,
    };

    if !valid_pair(&query.pair) {
        return bad_request("invalid pair");
    }
    if !valid_timeframe(&query.timeframe) {
        return bad_request("invalid timeframe");
    }
    if query.strategy_toml.len() > MAX_STRATEGY_TOML_BYTES {
        return bad_request(format!(
            "strategy_toml too large (max {MAX_STRATEGY_TOML_BYTES} bytes)"
        ));
    }

    let params = BacktestParams {
        market_type,
        pair: &query.pair,
        timeframe: &query.timeframe,
        strategy_toml: &query.strategy_toml,
        initial_balance: query.initial_balance.unwrap_or(10_000.0),
        fee_bps: query.fee_bps.unwrap_or(0.0),
        slippage_bps: query.slippage_bps.unwrap_or(0.0),
        before: query.before,
        after: query.after,
    };

    match super::strategy::run_backtest(&state, &params) {
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

/// 组装路由。
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
        assert_eq!(parse_market_type(None).unwrap(), MarketType::Futures);
        assert_eq!(
            parse_market_type(Some("futures")).unwrap(),
            MarketType::Futures
        );
        assert_eq!(parse_market_type(Some("spot")).unwrap(), MarketType::Spot);
        assert!(parse_market_type(Some("invalid")).is_err());
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
    async fn test_klines_rejects_oversized_limit() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/klines?market_type=futures&pair=BTCUSDT&timeframe=1h&limit=999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_strategy_rejects_oversized_toml() {
        let app = test_app();
        let big = "x".repeat(MAX_STRATEGY_TOML_BYTES + 1);
        let uri = format!(
            "/api/strategy?market_type=futures&pair=BTCUSDT&timeframe=1h&strategy_toml={big}"
        );
        let res = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_market_types_lists_enum_variants() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/market-types")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["types"][0], "futures");
        assert_eq!(json["data"]["types"][1], "spot");
    }

    #[tokio::test]
    async fn test_health_ok() {
        let app = test_app();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
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
