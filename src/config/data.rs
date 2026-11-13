//! 数据源与扫描范围配置。

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;

/// 单值或列表, 统一归一化为 `Vec<String>`。
/// TOML 里 `symbol = "BTCUSDT"` 与 `symbol = ["BTCUSDT", "ETHUSDT"]` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 单值或列表, 统一归一化。TOML 里 `symbol = "BTC"` 与 `symbol = ["BTC","ETH"]` 都合法。
pub enum OneOrMany {
    /// 单值形态。
    One(String),
    /// 列表形态。
    Many(Vec<String>),
}

impl OneOrMany {
    /// 归一化后的品种/周期列表(**借用**)。
    ///
    /// 收 `&self` 返回 `&[String]` 而不是克隆一份 `Vec<String>`: 调用方
    /// (`Config::validate` / `app::run`) 每次都要用这个列表, 而克隆会让
    /// 每次调用多分配一条与列表等长的向量。
    #[must_use]
    pub fn items(&self) -> &[String] {
        match self {
            OneOrMany::One(s) => std::slice::from_ref(s),
            OneOrMany::Many(v) => v.as_slice(),
        }
    }
}

/// vta 自己的数据配置: 支持 symbol/timeframe 列表。
/// 加载时通过 [`DataSection::for_pair`] 降为单值形态。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSection {
    /// 数据源类型(如 `binance`)。
    #[serde(rename = "type")]
    pub source_type: crate::data::DataSource,
    /// Binance 市场(spot/futures/um/futures/cm), 缺省 `futures/um`。
    #[serde(default)]
    pub market: Option<crate::data::Market>,
    /// 数据根目录。
    pub dir: PathBuf,
    /// 品种, 单值或列表。
    pub symbol: OneOrMany,
    /// 周期, 单值或列表。
    pub timeframe: OneOrMany,
    #[serde(default)]
    /// 起始日期(含), 缺省不限。
    pub start_date: Option<String>,
    #[serde(default)]
    /// 结束日期(含), 缺省不限。
    pub end_date: Option<String>,
}

impl DataSection {
    /// 品种列表。
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        self.symbol.items()
    }

    /// 周期列表。
    #[must_use]
    pub fn timeframes(&self) -> &[String] {
        self.timeframe.items()
    }

    /// 取出一组 (symbol, timeframe) 对应的单值配置。
    /// 已验证构造：非法 symbol/timeframe 即 `Err`，调用方按跳过处理（不 `unwrap`）。
    ///
    /// # Errors
    /// 品种/周期/日期非法，或给 Dukascopy 指定 `market` 时返回错误；
    /// 调用方按"跳过该 (品种, 周期) 对"处理。
    pub fn for_pair(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> Result<crate::data::DataConfig, AppError> {
        crate::data::DataConfig::new(
            self.source_type,
            self.market,
            &self.dir,
            symbol,
            timeframe,
            self.start_date.as_deref(),
            self.end_date.as_deref(),
        )
    }
}
