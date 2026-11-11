//! 库层统一错误类型。
//!
//! 分层约定（`err-thiserror-lib` / `err-anyhow-app`）：
//! - 库（`lib.rs`）内的所有可恢复失败都收敛到 [`AppError`]，用 `thiserror` 表达；
//! - 二进制入口（`main.rs`）用 `anyhow` 收敛，仅做打印，不参与类型判断。
//!
//! [`AppError`] 不提供 `From<anyhow::Error>`：`anyhow::Error` 不是
//! [`std::error::Error`]（无法提供 `source()`），把它压进本枚举会丢掉错误链，
//! 也会把任意来源的失败误标成"配置错误"。跨层转换交给 `anyhow` 的
//! `From<E: std::error::Error + Send + Sync>`  blanket impl。

use std::path::PathBuf;

use thiserror::Error;

/// 应用级错误。
///
/// 变体上的 `#[source]` 保留底层错误链，`Display` 只给出一句话摘要。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppError {
    /// 文件系统 IO 失败。
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// 配置语义错误（取值非法、目录缺失等）。
    #[error("config error: {0}")]
    Config(String),

    /// 读取配置文件失败。
    #[error("failed to read config file {path}")]
    ConfigRead {
        /// 配置文件路径。
        path: PathBuf,
        /// 底层 IO 错误。
        #[source]
        source: std::io::Error,
    },

    /// 解析配置文件失败（TOML 错误自带行列号）。
    #[error("failed to parse config file {path}")]
    ConfigParse {
        /// 配置文件路径。
        path: PathBuf,
        /// 底层 TOML 解析错误。
        #[source]
        source: toml::de::Error,
    },

    /// 行情数据加载失败。
    #[error("data error: {0}")]
    Data(#[from] crate::app::data_loader::DataError),

    /// 本地状态库（SQLite）失败。
    #[error("state error: {0}")]
    State(#[from] crate::app::state::StateError),

    /// 监听地址非法。
    #[error("invalid listen address {addr}")]
    InvalidAddr {
        /// 拼装出来的监听地址。
        addr: String,
        /// 底层地址解析错误。
        #[source]
        source: std::net::AddrParseError,
    },
}
