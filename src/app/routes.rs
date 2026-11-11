//! HTTP 路由：参数校验 → 数据加载 → 统一响应体。

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use super::data_loader::{DataError, DataLoader, MarketType, ParseMarketTypeError};
use super::state::{LayoutInput, StateError, StateStore};
use crate::utils::{valid_pair, valid_timeframe};

/// 单次 `/api/klines` 返回的最大 bar 数（防御超大响应体）。
const MAX_KLINE_LIMIT: usize = 10_000;

/// 服务共享状态。
#[derive(Clone)]
pub struct AppState {
    pub(crate) data_loader: Arc<DataLoader>,
    pub(crate) state_store: Arc<StateStore>,
}

impl AppState {
    /// 构造服务状态。
    pub fn new(data_dir: impl Into<PathBuf>, state_store: Arc<StateStore>) -> Self {
        Self {
            data_loader: Arc::new(DataLoader::new(data_dir)),
            state_store,
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

/// `/api/drawings` 查询参数。
#[derive(Debug, Deserialize)]
pub struct DrawingsQuery {
    /// 市场类型（缺省为 `futures`）。
    pub market_type: Option<String>,
    /// 交易对。
    pub pair: String,
}

/// `PUT /api/drawings` 请求体。
#[derive(Debug, Deserialize)]
pub struct DrawingsInput {
    /// 该交易对的完整绘图数组（整组替换）。
    pub drawings: serde_json::Value,
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

/// 构造 404 响应。
fn not_found(msg: impl Into<String>) -> ErrorResponse {
    (StatusCode::NOT_FOUND, Json(ApiResponse::err(msg)))
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

/// 构造 500 响应（内部细节一律脱敏）。
fn internal_error() -> ErrorResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::err("internal error")),
    )
}

/// 状态库错误 → HTTP 响应：输入问题 400，其余 500 且脱敏。
fn handle_state_error(e: StateError) -> ErrorResponse {
    match &e {
        StateError::Invalid(msg) => {
            tracing::warn!(error = %e, "invalid state input");
            bad_request(msg.clone())
        }
        _ => {
            tracing::error!(error = %e, "state store failed");
            internal_error()
        }
    }
}

/// 在 blocking 线程池执行 SQLite 操作。
async fn state_task<T, F>(f: F) -> Result<T, ErrorResponse>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StateError> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(handle_state_error(e)),
        Err(e) => {
            tracing::error!(error = %e, "state task join failed");
            Err(internal_error())
        }
    }
}

/// 布局 ID 只允许 UUID / 简单标识符字符，防御异常路径与超长输入。
fn valid_layout_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `GET /api/layouts`：布局列表（不含 payload）。
#[tracing::instrument(skip_all)]
async fn list_layouts_handler(State(state): State<AppState>) -> Response {
    let store = state.state_store.clone();
    match state_task(move || store.list_layouts()).await {
        Ok(layouts) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({ "layouts": layouts }))),
        )
            .into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `POST /api/layouts`：创建命名布局。
#[tracing::instrument(skip_all)]
async fn create_layout_handler(
    State(state): State<AppState>,
    Json(input): Json<LayoutInput>,
) -> Response {
    let store = state.state_store.clone();
    match state_task(move || store.create_layout(input)).await {
        Ok(layout) => (StatusCode::CREATED, Json(ApiResponse::ok(layout))).into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `GET /api/layouts/{id}`：读取完整布局。
#[tracing::instrument(skip_all, fields(id = %id))]
async fn get_layout_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if !valid_layout_id(&id) {
        return bad_request("invalid layout id").into_response();
    }
    let store = state.state_store.clone();
    match state_task(move || store.get_layout(&id)).await {
        Ok(Some(layout)) => (StatusCode::OK, Json(ApiResponse::ok(layout))).into_response(),
        Ok(None) => not_found("layout not found").into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `PUT /api/layouts/{id}`：更新布局。
#[tracing::instrument(skip_all, fields(id = %id))]
async fn update_layout_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<LayoutInput>,
) -> Response {
    if !valid_layout_id(&id) {
        return bad_request("invalid layout id").into_response();
    }
    let store = state.state_store.clone();
    match state_task(move || store.update_layout(&id, input)).await {
        Ok(Some(layout)) => (StatusCode::OK, Json(ApiResponse::ok(layout))).into_response(),
        Ok(None) => not_found("layout not found").into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `DELETE /api/layouts/{id}`：删除布局。
#[tracing::instrument(skip_all, fields(id = %id))]
async fn delete_layout_handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if !valid_layout_id(&id) {
        return bad_request("invalid layout id").into_response();
    }
    let store = state.state_store.clone();
    match state_task(move || store.delete_layout(&id)).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({ "deleted": true }))),
        )
            .into_response(),
        Ok(false) => not_found("layout not found").into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `GET /api/drawings`：读取某交易对的绘图组。
#[tracing::instrument(skip_all, fields(pair = %query.pair))]
async fn get_drawings_handler(
    State(state): State<AppState>,
    Query(query): Query<DrawingsQuery>,
) -> Response {
    let market_type = match require_market_type(query.market_type.as_deref()) {
        Ok(mt) => mt,
        Err(resp) => return resp.into_response(),
    };
    if !valid_pair(&query.pair) {
        return bad_request("invalid pair").into_response();
    }

    let market = market_type.to_string();
    let pair = query.pair;
    let store = state.state_store.clone();
    let market_task = market.clone();
    let pair_task = pair.clone();
    match state_task(move || store.get_drawings(&market_task, &pair_task)).await {
        Ok(drawings) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market,
                "pair": pair,
                "drawings": drawings
            }))),
        )
            .into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `PUT /api/drawings`：整组替换某交易对的绘图。
#[tracing::instrument(skip_all, fields(pair = %query.pair))]
async fn put_drawings_handler(
    State(state): State<AppState>,
    Query(query): Query<DrawingsQuery>,
    Json(input): Json<DrawingsInput>,
) -> Response {
    let market_type = match require_market_type(query.market_type.as_deref()) {
        Ok(mt) => mt,
        Err(resp) => return resp.into_response(),
    };
    if !valid_pair(&query.pair) {
        return bad_request("invalid pair").into_response();
    }

    let market = market_type.to_string();
    let pair = query.pair;
    let doc = input.drawings;
    let store = state.state_store.clone();
    let market_task = market.clone();
    let pair_task = pair.clone();
    match state_task(move || store.replace_drawings(&market_task, &pair_task, &doc)).await {
        Ok(drawings) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "market_type": market,
                "pair": pair,
                "drawings": drawings
            }))),
        )
            .into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `GET /api/preferences`：读取全部偏好。
#[tracing::instrument(skip_all)]
async fn get_preferences_handler(State(state): State<AppState>) -> Response {
    let store = state.state_store.clone();
    match state_task(move || store.get_preferences()).await {
        Ok(preferences) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "preferences": preferences
            }))),
        )
            .into_response(),
        Err(resp) => resp.into_response(),
    }
}

/// `PUT /api/preferences`：合并写入偏好。
#[tracing::instrument(skip_all)]
async fn put_preferences_handler(
    State(state): State<AppState>,
    Json(patch): Json<serde_json::Map<String, serde_json::Value>>,
) -> Response {
    let store = state.state_store.clone();
    match state_task(move || store.put_preferences(patch)).await {
        Ok(preferences) => (
            StatusCode::OK,
            Json(ApiResponse::ok(serde_json::json!({
                "preferences": preferences
            }))),
        )
            .into_response(),
        Err(resp) => resp.into_response(),
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
        .route(
            "/api/layouts",
            axum::routing::get(list_layouts_handler).post(create_layout_handler),
        )
        .route(
            "/api/layouts/{id}",
            axum::routing::get(get_layout_handler)
                .put(update_layout_handler)
                .delete(delete_layout_handler),
        )
        .route(
            "/api/drawings",
            axum::routing::get(get_drawings_handler).put(put_drawings_handler),
        )
        .route(
            "/api/preferences",
            axum::routing::get(get_preferences_handler).put(put_preferences_handler),
        )
        .route(
            "/udf/config",
            axum::routing::get(super::udf::config_handler),
        )
        .route("/udf/time", axum::routing::get(super::udf::time_handler))
        .route(
            "/udf/symbols",
            axum::routing::get(super::udf::symbols_handler),
        )
        .route(
            "/udf/search",
            axum::routing::get(super::udf::search_handler),
        )
        .route(
            "/udf/history",
            axum::routing::get(super::udf::history_handler),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
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
        let store = Arc::new(StateStore::open_in_memory().unwrap());
        create_router(AppState::new(PathBuf::new(), store))
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

    async fn json_body(res: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_layout_crud() {
        let app = test_app();
        let create = serde_json::json!({
            "name": "alpha",
            "market_type": "futures",
            "pair": "BTCUSDT",
            "timeframe": "1h",
            "payload": {"indicators": [], "viewport": null}
        });
        let res = app
            .clone()
            .oneshot(json_request("POST", "/api/layouts", create))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = json_body(res).await;
        assert_eq!(body["data"]["name"], "alpha");
        let id = body["data"]["id"].as_str().unwrap().to_string();

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/layouts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["data"]["layouts"].as_array().unwrap().len(), 1);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/layouts/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["data"]["payload"]["indicators"], serde_json::json!([]));

        let update = serde_json::json!({
            "name": "beta",
            "market_type": "futures",
            "pair": "ETHUSDT",
            "timeframe": "4h",
            "payload": {"indicators": [{"id": "ema"}]}
        });
        let res = app
            .clone()
            .oneshot(json_request("PUT", &format!("/api/layouts/{id}"), update))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["data"]["name"], "beta");
        assert_eq!(body["data"]["pair"], "ETHUSDT");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/layouts/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/layouts/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_layout_rejects_invalid_fields() {
        let app = test_app();
        let create = serde_json::json!({
            "name": "bad",
            "market_type": "futures",
            "pair": "../etc",
            "timeframe": "1h",
            "payload": {}
        });
        let res = app
            .oneshot(json_request("POST", "/api/layouts", create))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_drawings_roundtrip() {
        let app = test_app();
        let doc = serde_json::json!({
            "drawings": [{"id": "a", "points": [{"time": 1, "price": 2}]}]
        });
        let res = app
            .clone()
            .oneshot(json_request(
                "PUT",
                "/api/drawings?market_type=futures&pair=BTCUSDT",
                doc,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["data"]["drawings"].as_array().unwrap().len(), 1);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/drawings?market_type=futures&pair=BTCUSDT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["data"]["drawings"][0]["id"], "a");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/drawings?market_type=futures&pair=ETHUSDT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = json_body(res).await;
        assert_eq!(body["data"]["drawings"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_drawings_reject_invalid_payload_and_pair() {
        let app = test_app();
        let res = app
            .clone()
            .oneshot(json_request(
                "PUT",
                "/api/drawings?market_type=futures&pair=BTCUSDT",
                serde_json::json!({"drawings": {}}),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let res = app
            .oneshot(json_request(
                "PUT",
                "/api/drawings?market_type=futures&pair=..%2Fetc",
                serde_json::json!({"drawings": []}),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_preferences_merge() {
        let app = test_app();
        let res = app
            .clone()
            .oneshot(json_request(
                "PUT",
                "/api/preferences",
                serde_json::json!({"last_session": {"pair": "BTCUSDT"}}),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(json_request(
                "PUT",
                "/api/preferences",
                serde_json::json!({"theme": "dark"}),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/preferences")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(
            body["data"]["preferences"]["last_session"]["pair"],
            "BTCUSDT"
        );
        assert_eq!(body["data"]["preferences"]["theme"], "dark");
    }
}
