use vta::app;
use vta::cli;
use vta::config;

fn main() -> anyhow::Result<()> {
    let cli = cli::parse();

    let config_path = &cli.config;
    let config = config::load_config(config_path)?;

    app::run(&config)?;

    Ok(())
}
