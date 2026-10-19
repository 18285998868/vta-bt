use thiserror::Error;

/// 顶层应用错误: 统一 `io / 配置 / 数据 / parquet / arrow` 及其它消息。
#[derive(Debug, Error)]
/// 顶层应用错误。
///
/// 变体 `Io` / `Config` / `Data` / `Parquet` / `Arrow` 均由 `#[from]` 自动
/// 实现 `From`, 因此底层错误可用 `?` 直接上抛且保留 `source` 链;
/// [`AppError::Msg`] 用于无法归入上述类别的语义错误。
pub enum AppError {
    #[error("io error: {0}")]
    /// 文件系统 IO 失败。
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    /// TOML 解析失败(自带行列号)。
    Config(#[from] toml::de::Error),
    #[error("data error: {0}")]
    /// 行情数据加载失败。
    Data(#[from] kline_reader::ReaderError),
    #[error("parquet error: {0}")]
    /// Parquet 读写失败。
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("arrow error: {0}")]
    /// Arrow 数组/schema 构造失败。
    Arrow(#[from] arrow::error::ArrowError),
    #[error("{0}")]
    /// 无法归入上述类别的语义错误(消息式)。
    Msg(String),
}

/// 本 crate 的 [`Result`] 别名。
pub type Result<T> = std::result::Result<T, AppError>;
