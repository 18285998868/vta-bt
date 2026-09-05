use thiserror::Error;

/// 顶层应用错误: 统一 `io / 配置 / 数据 / parquet / arrow` 及其它消息。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    Config(#[from] toml::de::Error),
    #[error("data error: {0}")]
    Data(#[from] lib_reader::ReaderError),
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
