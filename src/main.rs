use clap::Parser;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tradingview_backend=info,tower_http=info".into()),
        )
        .init();

    let args = tradingview_backend::Args::parse();
    if let Err(e) = tradingview_backend::run(args).await {
        tracing::error!("{e:#}");
        std::process::exit(1);
    }
}
