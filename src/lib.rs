pub mod data_loader;
pub mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use clap::Parser;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

pub use routes::{AppState, create_router};

#[derive(Parser, Debug)]
#[command(name = "tradingview-backend")]
#[command(about = "TradingView K\u{7ebf}\u{56fe}\u{672c}\u{5730}\u{670d}\u{52a1}")]
pub struct Args {
    /// Listening address.
    #[arg(long, default_value = "127.0.0.1", env = "HOST")]
    pub host: String,

    /// Listening port.
    #[arg(short, long, default_value = "3001", env = "PORT")]
    pub port: u16,

    /// Directory containing parquet kline data.
    ///
    /// When not set, the binary locates `data/` relative to its own location.
    #[arg(long, env = "DATA_DIR")]
    pub data_dir: Option<String>,
}

/// Resolve the default data directory at runtime.
///
/// Priority:
/// 1. `data/` next to the executable
/// 2. `data/` in the parent of the executable directory (for `target/debug/` layout)
/// 3. `data/` in the current working directory
fn get_default_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let candidate = exe_dir.join("data");
        if candidate.is_dir() {
            return candidate;
        }
        if let Some(parent) = exe_dir.parent() {
            let candidate = parent.join("data");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("data")
}

/// Resolve the frontend static directory at runtime.
///
/// Priority:
/// 1. `frontend/dist/` next to the executable
/// 2. `frontend/dist/` in the parent of the executable directory
/// 3. `frontend/dist/` in the current working directory
fn get_frontend_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let candidate = exe_dir.join("frontend").join("dist");
        if candidate.is_dir() {
            return candidate;
        }
        if let Some(parent) = exe_dir.parent() {
            let candidate = parent.join("frontend").join("dist");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("frontend/dist")
}

fn setup_cors() -> CorsLayer {
    tracing::warn!("CORS allows all origins — configure a specific origin for production use");
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let data_dir = args
        .data_dir
        .map(PathBuf::from)
        .unwrap_or_else(get_default_data_dir);

    if !data_dir.is_dir() {
        anyhow::bail!("data directory not found: {:?}", data_dir);
    }

    let app_state = AppState::new(data_dir);

    let frontend_dir = get_frontend_dir();
    if !frontend_dir.is_dir() {
        tracing::warn!(path = %frontend_dir.display(), "frontend dist directory not found");
    }

    let app: Router = create_router(app_state)
        .layer(setup_cors())
        .fallback_service(ServeDir::new(&frontend_dir).append_index_html_on_directories(true));

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    tracing::info!("TradingView Backend starting on http://{}", addr);
    tracing::info!("API Endpoints:");
    tracing::info!("  GET /api/health");
    tracing::info!("  GET /api/market-types");
    tracing::info!("  GET /api/pairs?market_type=futures|spot");
    tracing::info!("  GET /api/timeframes?market_type=...&pair=...");
    tracing::info!("  GET /api/klines?market_type=...&pair=...&timeframe=...&limit=...");
    tracing::info!("Frontend: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
