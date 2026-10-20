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
    pub(crate) strategy_cache: moka::sync::Cache<u64, Arc<super::strategy::StrategyResult>>,
    /// 回测并发闸门：同时在跑的回测数上限。
    ///
    /// 回测是重 CPU 活（59k 根 ≈ 134ms），且每档网格还要再乘一遍。
    /// 不设闸门时，一个请求能吃满几百 MB 内存。
    pub(crate) strategy_gate: Arc<tokio::sync::Semaphore>,
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
            strategy_gate: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_BACKTESTS)),
        }
    }
}

/// 同时在跑的回测数上限。
const MAX_CONCURRENT_BACKTESTS: usize = 4;

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
            types: MarketType::SUPPORTED
                .iter()
                .map(|m| m.to_string())
                .collect(),
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
        DataError::InvalidParam(_) => {
            tracing::warn!(error = %e, "invalid parameter");
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(e.to_string())),
            )
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

    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        move || loader.list_pairs(market_type)
    })
    .await;

    match joined {
        Ok(Ok(pairs)) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pairs": pairs
            }))),
        ),
        Ok(Err(e)) => handle_data_error(e),
        Err(e) => handle_data_error(DataError::Task(format!("list pairs task: {e}"))),
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

    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        let pair = query.pair.clone();
        move || loader.list_timeframes(market_type, &pair)
    })
    .await;

    match joined {
        Ok(Ok(timeframes)) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframes": timeframes
            }))),
        ),
        Ok(Err(e)) => handle_data_error(e),
        Err(e) => handle_data_error(DataError::Task(format!("list timeframes task: {e}"))),
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

    // 读盘 + 解析是重同步 IO：放 blocking 线程池，别占住 tokio worker。
    // 闭包必须是 'static ⇒ 先把要用��的值拷出来再 move。
    let before = query.before;
    let after = query.after;
    let limit = query.limit;
    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        let pair = query.pair.clone();
        let timeframe = query.timeframe.clone();
        move || loader.load_klines_filtered(market_type, &pair, &timeframe, before, after, limit)
    })
    .await;

    match joined {
        Ok(Ok(klines)) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market_type.to_string(),
                "pair": query.pair,
                "timeframe": query.timeframe,
                "count": klines.len(),
                "klines": klines
            }))),
        ),
        Ok(Err(e)) => handle_data_error(e),
        Err(e) => handle_data_error(DataError::Task(format!("kline load task: {e}"))),
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

    let initial_balance = query.initial_balance.unwrap_or(10_000.0);
    let fee_bps = query.fee_bps.unwrap_or(0.0);
    let slippage_bps = query.slippage_bps.unwrap_or(0.0);
    let before = query.before;
    let after = query.after;

    // 回测是重 CPU 活（59k 根 ≈ 134ms/档）：放 blocking 线程池，别占住 tokio worker。
    //
    // 并发闸门：同时在跑的回测数封顶，否则内存与 CPU 都会被单个请求吃光
    // （每个回测持有一份 59k×6 的列 + 信号 + 资金曲线）。
    // `_permit` 活到函数结束 ⇒ 响应构造完才释放名额。
    let _permit = match state.strategy_gate.clone().acquire_owned().await {
        Ok(p) => p,
        Err(_) => return strategy_error(StrategyError::Internal("backtest gate closed".into())),
    };

    // 闭包必须 'static ⇒ 查询串要先 own 住（`BacktestParams` 借用它们）。
    let joined = tokio::task::spawn_blocking({
        let state = state.clone();
        let pair = query.pair.clone();
        let timeframe = query.timeframe.clone();
        let strategy_toml = query.strategy_toml.clone();
        move || {
            let params = BacktestParams {
                market_type,
                pair: &pair,
                timeframe: &timeframe,
                strategy_toml: &strategy_toml,
                initial_balance,
                fee_bps,
                slippage_bps,
                before,
                after,
            };
            super::strategy::run_backtest(&state, &params)
        }
    })
    .await;

    match joined {
        Ok(Ok(result)) => (
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
                "summary": result.summary,
                "param_grid": result.param_grid,
                "runs": result.runs
            }))),
        ),
        Ok(Err(e)) => strategy_error(e),
        Err(e) => strategy_error(StrategyError::Internal(format!("backtest task: {e}"))),
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
    async fn test_market_types_lists_supported_variants() {
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
        // 只暴露**本地确有数据源**的类型：spot 无数据，不对外列。
        assert_eq!(json["data"]["types"][0], "futures");
        assert_eq!(json["data"]["types"][1], "dukascopy");
        assert_eq!(json["data"]["types"].as_array().unwrap().len(), 2);
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
