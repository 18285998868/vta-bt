//! `vta` 的命令行入口。
//!
//! 职责**仅限于**装配: 初始化日志订阅器、解析 CLI、把结果交给
//! [`vta::app::run`] 或 [`vta::pbo::run_pbo_report`]。业务逻辑全在库 crate
//! (`vta`) 里, 这样库可以被其它程序直接复用。

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
            app::run_with(
                &cfg,
                app::RunOptions {
                    no_cache: cli.no_cache,
                },
            )?;
        }
        (None, Some(dir)) => {
            vta::pbo::run_pbo_report(&dir)?;
        }
        _ => anyhow::bail!("必须二选一: `-c <config>` 回测, 或 `--pbo <reports_dir>` 实算 PBO"),
    }

    Ok(())
}
