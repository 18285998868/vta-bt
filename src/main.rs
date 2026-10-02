use tracing_subscriber::EnvFilter;

use vta::app;
use vta::cli;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = cli::parse();

    match (cli.config, cli.pbo) {
        (Some(cfg), None) => {
            app::run(&cfg)?;
        }
        (None, Some(dir)) => {
            vta::pbo::run_pbo_report(&dir)?;
        }
        _ => anyhow::bail!("必须二选一: `-c <config>` 回测, 或 `--pbo <reports_dir>` 实算 PBO"),
    }

    Ok(())
}
