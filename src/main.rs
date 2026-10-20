//! 入口：加载 `.env` → 初始化 tracing → 读配置 → 启动服务。
//!
//! 错误在此收敛为 [`anyhow::Error`]（应用层约定）：库层用
//! [`tradingview_backend::AppError`] 表达可恢复失败，入口只负责打印。

use anyhow::Context as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 先读 `.env`（可能提供 RUST_LOG），再装 subscriber；结果留到装完之后再记，
    // 否则这条 debug 会被自己丢掉。
    let dotenv = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("tradingview_backend=info,tower_http=info")
            }),
        )
        .init();

    if let Err(e) = dotenv {
        tracing::debug!("dotenv not loaded: {e}");
    }

    let config = tradingview_backend::config::Config::load().context("load config")?;
    tradingview_backend::run(&config)
        .await
        .context("run server")?;
    Ok(())
}
