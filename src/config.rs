//! 服务配置：固定路径 TOML → 强类型配置 → 语义校验。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;

/// 相对 crate 根的配置文件路径。
const CONFIG_REL_PATH: &str = "config/config.toml";

/// 服务配置。
///
/// `#[serde(default)]` 让缺失字段回落到 [`Config::default`]，
/// `deny_unknown_fields` 让拼错的键直接报错而不是被静默忽略。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// 监听主机名或 IP。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    /// 行情数据根目录（kline-reader 按数据源自动追加子路径）。
    pub data_dir: PathBuf,
    /// 前端静态资源目录；缺失时服务降级为纯 API 模式。
    pub frontend_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3001,
            data_dir: PathBuf::new(),
            frontend_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist"),
        }
    }
}

impl Config {
    /// 从固定路径 `config/config.toml` 读取配置。
    ///
    /// # Errors
    /// - 文件不存在或不可读 → [`AppError::ConfigRead`]
    /// - TOML 语法/字段非法 → [`AppError::ConfigParse`]
    /// - `data_dir` 为空 → [`AppError::Config`]
    pub fn load() -> Result<Self, AppError> {
        Self::load_from(&default_config_path())
    }

    /// 从指定路径读取配置。
    ///
    /// # Errors
    /// 同 [`Config::load`]。
    pub fn load_from(path: &Path) -> Result<Self, AppError> {
        let content = std::fs::read_to_string(path).map_err(|source| AppError::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        let cfg: Self = toml::from_str(&content).map_err(|source| AppError::ConfigParse {
            path: path.to_path_buf(),
            source,
        })?;
        cfg.validate()
    }

    /// 语义校验：目前只有 `data_dir` 是必填。
    ///
    /// # Errors
    /// `data_dir` 为空时返回 [`AppError::Config`]。
    pub fn validate(self) -> Result<Self, AppError> {
        if self.data_dir.as_os_str().is_empty() {
            return Err(AppError::Config("data_dir must not be empty".into()));
        }
        Ok(self)
    }
}

/// 默认配置文件路径（编译期 crate 根 + 固定相对路径）。
fn default_config_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(CONFIG_REL_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_cfg(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn defaults_fill_missing_fields() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write_cfg(dir.path(), "data_dir = \"/tmp/data\"\n");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 3001);
        assert_eq!(cfg.data_dir, PathBuf::from("/tmp/data"));
        assert!(cfg.frontend_dir.ends_with("frontend/dist"));
    }

    #[test]
    fn rejects_empty_data_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write_cfg(dir.path(), "data_dir = \"\"\n");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert_eq!(err.to_string(), "config error: data_dir must not be empty");
    }

    #[test]
    fn rejects_unknown_field() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write_cfg(dir.path(), "data_dir = \"/tmp/data\"\ntypo = 1\n");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, AppError::ConfigParse { .. }));
    }

    #[test]
    fn missing_file_reports_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("nope.toml");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, AppError::ConfigRead { .. }));
        // source 链保留底层 io 错误
        assert!(std::error::Error::source(&err).is_some());
    }
}
