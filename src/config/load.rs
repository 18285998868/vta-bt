//! 读取并解析 TOML 配置文件。

use super::Config;

/// 读取并解析 TOML 配置文件。
///
/// # Errors
/// 文件读取失败, 或 TOML 结构非法(含未知字段 / 类型不匹配)时返回
/// `AppError`。
///
/// 收 [`Path`](std::path::Path) 而非 `&str`: 配置路径来自 CLI, 非 UTF-8 路径在 `&str` 边界上
/// 会被迫 `to_str().expect(..)` —— 那是把用户输入的问题变成一次 panic。
pub fn load_config(path: &std::path::Path) -> crate::error::Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
