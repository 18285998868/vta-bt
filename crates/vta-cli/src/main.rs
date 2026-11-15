//! `vta` 的命令行入口。
//!
//! 职责**仅限于**装配: 初始化日志订阅器、解析 CLI、把结果交给
//! [`vta::app::run`]、[`vta_pbo::run_pbo_report`] 或 `vta_analysis`。
//! 业务逻辑全在库 crate (`vta`) 里, 这样库可以被其它程序直接复用。

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

    match (cli.config, cli.pbo, cli.analyze) {
        (Some(cfg), None, None) => {
            app::run_with(
                &cfg,
                app::RunOptions {
                    no_cache: cli.no_cache,
                },
            )?;
        }
        (None, Some(dir), None) => {
            vta_pbo::run_pbo_report(&dir)?;
        }
        (None, None, Some(dir)) => {
            let out = cli.out.unwrap_or_else(|| dir.join("analysis"));
            let opts = vta_analysis::AnalyzeOptions {
                alpha: cli.alpha,
                bootstrap: cli.bootstrap,
                block_len: cli.block_len,
                seed: cli.seed,
                top: cli.top,
                min_trades: cli.min_trades,
                trials: cli.trials,
                max_curves: cli.max_curves,
                require_detail: cli.require_detail,
            };
            let report = vta_analysis::analyze(&dir, &opts)?;
            vta_analysis::write_report(&report, &out)?;
            tracing::info!(
                rows = report.coverage.total_rows,
                groups = report.coverage.groups,
                rank_eligible = report.coverage.rank_eligible,
                out = %out.display(),
                "分析完成"
            );
        }
        _ => anyhow::bail!(
            "必须三选一: `-c <config>` 回测, `--pbo <reports_dir>` 实算 PBO, \
             或 `--analyze <reports_dir>` 只读分析报告"
        ),
    }

    Ok(())
}
