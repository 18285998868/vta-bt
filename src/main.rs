//! 入口：加载 `.env` → 初始化 tracing → 读配置 → 启动服务。
//!
//! 错误在此收敛为 [`anyhow::Error`]（应用层约定）：库层用
//! [`tradingview_backend::AppError`] 表达可恢复失败，入口只负责打印。

use anyhow::Context as _;
use tracing_subscriber::EnvFilter;

/// 日志过滤优先级：应用专用的 `TRADINGVIEW_LOG` > 通用的 `RUST_LOG` > 默认。
///
/// 为什么要应用专用变量优先：`RUST_LOG` 是**全局**约定，很容易被环境里
/// 别人的设置（比如上层目录的 `.env`）无声覆盖 —— 而服务自己的日志级别
/// 应该是服务自己的决定。
const LOG_ENV_CANDIDATES: [&str; 2] = ["TRADINGVIEW_LOG", "RUST_LOG"];

/// 默认日志过滤（`TRADINGVIEW_LOG` / `RUST_LOG` 都未设置时）。
const LOG_FILTER_DEFAULT: &str = "tradingview_backend=info,tower_http=info";

/// 决定日志过滤规则。
fn log_filter() -> EnvFilter {
    for var in LOG_ENV_CANDIDATES {
        match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => return EnvFilter::new(v),
            _ => {}
        }
    }
    EnvFilter::new(LOG_FILTER_DEFAULT)
}

/// 加载当前工作目录下的 `.env`（**不向上遍历目录树**）。
///
/// `dotenvy::dotenv()` 会从 CWD 一路往上找 `.env`，于是仓库根目录的
/// `.env`（比如其中的 `RUST_LOG=debug`）会无声生效，服务代码里写的
/// 默认过滤规则形同虚设。只认 CWD 下的 `.env` 行为可预测。
///
/// 用 `from_path`（非 `from_path_override`）：**不覆盖**已存在的环境变量，
/// 命令行给的值优先于 `.env`。
fn load_dotenv() -> Option<dotenvy::Error> {
    let cwd = std::env::current_dir().ok()?;
    dotenvy::from_path(cwd.join(".env")).err()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 先读 `.env`（可能提供 RUST_LOG），再装 subscriber；结果留到装完之后再记，
    // 否则这条 debug 会被自己丢掉。
    let dotenv_err = load_dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .init();

    if let Some(e) = dotenv_err {
        tracing::debug!("dotenv not loaded: {e}");
    }

    let config = tradingview_backend::config::Config::load().context("load config")?;
    tradingview_backend::run(&config)
        .await
        .context("run server")?;
    Ok(())
}
