//! 服务编排：校验 → 状态 → 路由 → 监听 → 优雅停机。

pub mod data_loader;
pub mod routes;
pub mod state;
pub mod strategy;
pub mod udf;

pub use routes::AppState;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::error::AppError;

/// CORS 层。
///
/// `origins` 为空 = 全放行（仅本地开发，会打 warn）；非空 = 只放行列表内的
/// origin，并回显匹配的 origin。
fn setup_cors(origins: &[String]) -> CorsLayer {
    let layer = CorsLayer::new().allow_methods(Any).allow_headers(Any);
    if origins.is_empty() {
        tracing::warn!("CORS allows all origins — set `cors_origins` in config for production use");
        return layer.allow_origin(Any);
    }

    let mut allowed: Vec<axum::http::HeaderValue> = Vec::with_capacity(origins.len());
    for origin in origins {
        match axum::http::HeaderValue::from_str(origin) {
            Ok(v) => allowed.push(v),
            Err(e) => tracing::error!(origin, error = %e, "invalid CORS origin, ignored"),
        }
    }
    tracing::info!(
        count = allowed.len(),
        "CORS restricted to configured origins"
    );
    layer.allow_origin(allowed)
}

/// 等待停机信号（Ctrl-C / SIGTERM）。
///
/// 收到信号后**不立刻退出**：axum 会停止接受新连接，并等在途请求处理完
/// （`with_graceful_shutdown`），避免把正在跑的回测/下载硬切断。
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to install ctrl_c handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => tracing::error!(error = %e, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received, draining in-flight requests");
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

    let db_path = config
        .db_path
        .clone()
        .unwrap_or_else(|| data_dir.join("tradingview.db"));
    let state_store = std::sync::Arc::new(state::StateStore::open(&db_path)?);
    let app_state = AppState::new(data_dir.clone(), state_store);

    let frontend_dir = config.frontend_dir.clone();
    if !frontend_dir.is_dir() {
        tracing::warn!(path = %frontend_dir.display(), "frontend dist not found, API-only mode");
    }

    let app: Router = routes::create_router(app_state)
        .layer(setup_cors(&config.cors_origins))
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
    tracing::info!(path = %db_path.display(), "State db");
    tracing::info!(path = %frontend_dir.display(), "Frontend dir");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}
