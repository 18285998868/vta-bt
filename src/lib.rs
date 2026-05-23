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
#[command(about = "TradingView K线图本地服务")]
pub struct Args {
    /// 监听地址
    #[arg(long, default_value = "127.0.0.1", env = "HOST")]
    pub host: String,

    /// 监听端口
    #[arg(short, long, default_value = "3001", env = "PORT")]
    pub port: u16,

    /// 数据目录
    #[arg(long, env = "DATA_DIR")]
    pub data_dir: Option<String>,
}

fn get_default_data_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if dir.join("src").exists() {
        dir.join("..").join("data")
    } else {
        dir.join("data")
    }
}

fn setup_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub async fn run(args: Args) {
    let data_dir = args
        .data_dir
        .map(PathBuf::from)
        .unwrap_or_else(get_default_data_dir);

    if !data_dir.exists() {
        eprintln!("数据目录不存在: {:?}", data_dir);
        std::process::exit(1);
    }

    let app_state = AppState::new(data_dir);

    let frontend_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("frontend")
        .join("dist");

    let app: Router = create_router(app_state)
        .layer(setup_cors())
        .fallback_service(ServeDir::new(frontend_dir).append_index_html_on_directories(true));

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("Invalid address");

    tracing::info!("TradingView Backend starting on http://{}", addr);
    tracing::info!("API Endpoints:");
    tracing::info!("  GET /api/health");
    tracing::info!("  GET /api/market-types");
    tracing::info!("  GET /api/pairs?market_type=futures|spot");
    tracing::info!("  GET /api/timeframes?market_type=...&pair=...");
    tracing::info!("  GET /api/klines?market_type=...&pair=...&timeframe=...&limit=...");
    tracing::info!("Frontend: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
