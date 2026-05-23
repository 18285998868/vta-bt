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
    tradingview_backend::run(args).await;
}
