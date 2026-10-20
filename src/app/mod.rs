//! 服务编排：校验 → 状态 → 路由 → 监听。

pub mod data_loader;
pub mod routes;
pub mod strategy;

pub use routes::AppState;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::error::AppError;

/// CORS 层（全放行，仅供本地开发）。
fn setup_cors() -> CorsLayer {
    tracing::warn!("CORS allows all origins — configure a specific origin for production use");
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

/// 启动 HTTP 服务并常驻。
///
/// # Errors
/// - `data_dir` 不存在 → [`AppError::Config`]
/// - 监听地址非法 → [`AppError::InvalidAddr`]
/// - 绑定/服务失败 → [`AppError::Io`]
#[tracing::instrument(skip_all, fields(host = %config.host, port = config.port))]
pub async fn run(config: &Config) -> Result<(), AppError> {
    let data_dir = config.data_dir.clone();
    if !data_dir.is_dir() {
        return Err(AppError::Config(format!(
            "data directory not found: {}",
            data_dir.display()
        )));
    }

    let app_state = AppState::new(data_dir.clone());

    let frontend_dir = config.frontend_dir.clone();
    if !frontend_dir.is_dir() {
        tracing::warn!(path = %frontend_dir.display(), "frontend dist not found, API-only mode");
    }

    let app: Router = routes::create_router(app_state)
        .layer(setup_cors())
        .fallback_service(ServeDir::new(&frontend_dir).append_index_html_on_directories(true));

    let addr_str = format!("{}:{}", config.host, config.port);
    let addr =
        addr_str
            .parse::<std::net::SocketAddr>()
            .map_err(|source| AppError::InvalidAddr {
                addr: addr_str.clone(),
                source,
            })?;

    tracing::info!("TradingView Backend starting on http://{}", addr);
    tracing::info!(path = %data_dir.display(), "Data dir");
    tracing::info!(path = %frontend_dir.display(), "Frontend dir");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
