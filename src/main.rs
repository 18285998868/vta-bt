#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("tradingview_backend=info,tower_http=info")
            }),
        )
        .init();

    let config = tradingview_backend::config::Config::load()?;
    tradingview_backend::run(&config).await?;
    Ok(())
}
