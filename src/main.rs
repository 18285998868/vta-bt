use tracing_subscriber::EnvFilter;

use vta::app;
use vta::cli;
use vta::config;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = cli::parse();

    let config_path = &cli.config;
    let config = config::load_config(config_path)?;

    app::run(&config)?;

    Ok(())
}
