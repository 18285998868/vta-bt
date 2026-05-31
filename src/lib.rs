pub mod config;
pub mod data_loader;
pub mod error;
pub mod routes;

use std::path::PathBuf;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

pub use config::Config;
pub use routes::AppState;

fn setup_cors() -> CorsLayer {
    tracing::warn!("CORS allows all origins — configure a specific origin for production use");
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub async fn run(config: &Config) -> Result<(), error::AppError> {
    let data_dir = PathBuf::from(&config.data_dir);
    if !data_dir.is_dir() {
        return Err(error::AppError::Config(format!(
            "data directory not found: {:?}",
            data_dir
        )));
    }

    let app_state = AppState::new(data_dir.clone());

    let frontend_dir = PathBuf::from(&config.frontend_dir);
    if !frontend_dir.is_dir() {
        tracing::warn!(path = %frontend_dir.display(), "frontend dist not found, API-only mode");
    }

    let app: Router = routes::create_router(app_state)
        .layer(setup_cors())
        .fallback_service(ServeDir::new(&frontend_dir).append_index_html_on_directories(true));

    let addr = format!("{}:{}", config.host, config.port)
        .parse::<std::net::SocketAddr>()?;

    tracing::info!("TradingView Backend starting on http://{}", addr);
    tracing::info!("Data dir: {:?}", data_dir);
    tracing::info!("Frontend dir: {:?}", frontend_dir);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
