use thiserror::Error;

/// 顶层应用错误: 统一 `io / 配置 / 数据 / parquet / arrow` 及其它消息。
#[derive(Debug, Error)]
/// 顶层应用错误。
///
/// 变体 `Io` / `Config` / `BinanceData` / `DukascopyData` / `Parquet` / `Arrow`
/// 均由 `#[from]` 自动实现 `From`, 因此底层错误可用 `?` 直接上抛且保留 `source` 链;
/// [`AppError::Msg`] 用于无法归入上述类别的语义错误。
pub enum AppError {
    #[error("io error: {0}")]
    /// 文件系统 IO 失败。
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    /// TOML 解析失败(自带行列号)。
    Config(#[from] toml::de::Error),
    #[error("binance data error: {0}")]
    /// Binance 行情数据加载失败。
    BinanceData(#[from] binance_data_reader::ReaderError),
    #[error("dukascopy data error: {0}")]
    /// Dukascopy 行情数据加载失败。
    DukascopyData(#[from] dukascopy_data_reader::ReaderError),
    #[error("parquet error: {0}")]
    /// Parquet 读写失败。
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("arrow error: {0}")]
    /// Arrow 数组/schema 构造失败。
    Arrow(#[from] arrow::error::ArrowError),
    #[error("param skipped: {reason}")]
    /// 该**参数档位**不可计算(数据/参数不足, 例如 `period` 超过可用 bar 数)。
    ///
    /// **不是** bug: 它是"这一档参数在这份数据上跑不起来"的**预期**结果。
    /// 引擎会把它记成 [`crate::app::RunStatus::Invalid`] 并**继续**跑下一档,
    /// 而不是中断整轮回测 —— 但也**绝不**把它伪装成"正常运行但无信号"
    /// (那样它与"零波动低风险"的真实结论在数值上不可区分, 对参数网格
    /// 过拟合研究是致命的)。
    ParamSkipped {
        /// 可读原因(指标库的错误文本 / 未知参数名等)。
        reason: String,
    },
    #[error("{0}")]
    /// 无法归入上述类别的语义错误(消息式)。
    Msg(String),
}

/// 本 crate 的 [`Result`] 别名。
pub type Result<T> = std::result::Result<T, AppError>;
