//! 数据加载适配层：vta 自己的配置类型与两个 reader 的统一入口。
//!
//! reader 只负责加载、读取、清洗数据；本模块把两套 reader 的输出转换为 vta
//! 自己的 `KlineColumns`，并保留按数据源分派所需的 `DataSource`/`Market`。

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};

use crate::error::{AppError, Result};

/// 数据源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    /// Binance 公共归档。
    Binance,
    /// Dukascopy 公共归档。
    Dukascopy,
}

/// Binance 市场路径段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Market {
    /// 现货：`binance/data/spot/...`。
    Spot,
    /// USDⓈ-M 合约：`binance/data/futures/um/...`。
    #[default]
    Um,
    /// COIN-M 合约：`binance/data/futures/cm/...`。
    Cm,
}

impl Market {
    /// 规范路径段，例如 `futures/um`。
    #[must_use]
    pub const fn as_path(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Um => "futures/um",
            Self::Cm => "futures/cm",
        }
    }
}

impl fmt::Display for Market {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_path())
    }
}

impl FromStr for Market {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "spot" => Ok(Self::Spot),
            "futures/um" | "um" => Ok(Self::Um),
            "futures/cm" | "cm" => Ok(Self::Cm),
            other => Err(format!(
                "unsupported market: {other}, valid values: spot, futures/um, futures/cm"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Market {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug)]
enum Inner {
    Binance(binance_data_reader::KlineConfig),
    Dukascopy(dukascopy_data_reader::DataConfig),
}

/// 一组 (symbol, timeframe) 的加载配置。
///
/// 构造时即由对应 reader 完成全部校验，后续 [`Self::load`] 不再失败于配置。
#[derive(Debug)]
pub struct DataConfig {
    source: DataSource,
    market: Market,
    inner: Inner,
}

impl DataConfig {
    /// 构造单值配置。
    ///
    /// `market` 只对 Binance 有效；传给 Dukascopy 会返回 [`AppError::Msg`]。
    ///
    /// # Errors
    /// 品种/周期/日期非法，或给 Dukascopy 传了 `market` 时返回错误。
    pub fn new(
        source: DataSource,
        market: Option<Market>,
        dir: impl AsRef<Path>,
        symbol: &str,
        timeframe: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Self> {
        let dir = dir.as_ref();
        let (market, inner) = match source {
            DataSource::Binance => {
                let market = market.unwrap_or_default();
                let reader_market = match market {
                    Market::Spot => binance_data_reader::Market::Spot,
                    Market::Um => binance_data_reader::Market::Um,
                    Market::Cm => binance_data_reader::Market::Cm,
                };
                let cfg = binance_data_reader::KlineConfig::new_with_market(
                    reader_market,
                    dir,
                    symbol,
                    timeframe,
                    start_date,
                    end_date,
                )?;
                (market, Inner::Binance(cfg))
            }
            DataSource::Dukascopy => {
                if market.is_some() {
                    return Err(AppError::Msg("market is only supported for binance".into()));
                }
                let cfg = dukascopy_data_reader::DataConfig::new(
                    dir, symbol, timeframe, start_date, end_date,
                )?;
                (Market::Um, Inner::Dukascopy(cfg))
            }
        };
        Ok(Self {
            source,
            market,
            inner,
        })
    }

    /// 数据源。
    #[must_use]
    pub fn source_type(&self) -> DataSource {
        self.source
    }

    /// Binance 市场；Dukascopy 固定返回默认值。
    #[must_use]
    pub fn market(&self) -> Market {
        self.market
    }

    /// 数据根目录。
    #[must_use]
    pub fn dir(&self) -> &Path {
        match &self.inner {
            Inner::Binance(cfg) => cfg.dir(),
            Inner::Dukascopy(cfg) => cfg.dir(),
        }
    }

    /// 品种。
    #[must_use]
    pub fn symbol(&self) -> &str {
        match &self.inner {
            Inner::Binance(cfg) => cfg.symbol().as_str(),
            Inner::Dukascopy(cfg) => cfg.symbol().as_str(),
        }
    }

    /// 周期。
    #[must_use]
    pub fn timeframe(&self) -> &str {
        match &self.inner {
            Inner::Binance(cfg) => cfg.timeframe().as_str(),
            Inner::Dukascopy(cfg) => cfg.timeframe().as_str(),
        }
    }

    /// 起始日期（含）。
    #[must_use]
    pub fn start_date(&self) -> Option<NaiveDate> {
        match &self.inner {
            Inner::Binance(cfg) => cfg.start_date(),
            Inner::Dukascopy(cfg) => cfg.start_date(),
        }
    }

    /// 结束日期（含）。
    #[must_use]
    pub fn end_date(&self) -> Option<NaiveDate> {
        match &self.inner {
            Inner::Binance(cfg) => cfg.end_date(),
            Inner::Dukascopy(cfg) => cfg.end_date(),
        }
    }

    /// 按数据源调用对应 reader，并转换为 vta 的列结构。
    ///
    /// # Errors
    /// reader 的文件级失败或 IO 错误原样上抛。
    pub fn load(&self) -> Result<KlineColumns> {
        match &self.inner {
            Inner::Binance(cfg) => Ok(binance_data_reader::load_kline_columns(cfg)?.into()),
            Inner::Dukascopy(cfg) => Ok(dukascopy_data_reader::load_columns(cfg)?.into()),
        }
    }
}

/// vta 自己的 OHLCV 列结构。
#[derive(Debug, Clone)]
pub struct KlineColumns {
    time: Vec<i64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl KlineColumns {
    /// 构造等长列；长度不一致返回错误。
    ///
    /// # Errors
    /// 六列长度不一致时返回 [`AppError::Msg`]。
    pub fn new(
        time: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
        volume: Vec<f64>,
    ) -> Result<Self> {
        if time.len() != open.len()
            || time.len() != high.len()
            || time.len() != low.len()
            || time.len() != close.len()
            || time.len() != volume.len()
        {
            return Err(AppError::Msg("kline columns must have equal length".into()));
        }
        Ok(Self {
            time,
            open,
            high,
            low,
            close,
            volume,
        })
    }

    /// Bar 开盘时间（毫秒）。
    #[must_use]
    pub fn time(&self) -> &[i64] {
        &self.time
    }

    /// 开盘价。
    #[must_use]
    pub fn open(&self) -> &[f64] {
        &self.open
    }

    /// 最高价。
    #[must_use]
    pub fn high(&self) -> &[f64] {
        &self.high
    }

    /// 最低价。
    #[must_use]
    pub fn low(&self) -> &[f64] {
        &self.low
    }

    /// 收盘价。
    #[must_use]
    pub fn close(&self) -> &[f64] {
        &self.close
    }

    /// 成交量。
    #[must_use]
    pub fn volume(&self) -> &[f64] {
        &self.volume
    }

    /// Bar 数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.close.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }
}

impl From<binance_data_reader::KlineColumns> for KlineColumns {
    fn from(cols: binance_data_reader::KlineColumns) -> Self {
        let (time, open, high, low, close, volume) = cols.into_parts();
        Self {
            time,
            open,
            high,
            low,
            close,
            volume,
        }
    }
}

impl From<dukascopy_data_reader::KlineColumns> for KlineColumns {
    fn from(cols: dukascopy_data_reader::KlineColumns) -> Self {
        let (time, open, high, low, close, volume) = cols.into_parts();
        Self {
            time,
            open,
            high,
            low,
            close,
            volume,
        }
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_parses_aliases() {
        assert_eq!("spot".parse::<Market>(), Ok(Market::Spot));
        assert_eq!("um".parse::<Market>(), Ok(Market::Um));
        assert_eq!("futures/cm".parse::<Market>(), Ok(Market::Cm));
        assert!("nope".parse::<Market>().is_err());
    }

    #[test]
    fn columns_must_be_equal_length() {
        assert!(
            KlineColumns::new(
                vec![1],
                vec![1.0],
                vec![1.0],
                vec![1.0],
                vec![1.0],
                vec![1.0]
            )
            .is_ok()
        );
        assert!(
            KlineColumns::new(vec![1], vec![1.0], vec![1.0], vec![1.0], vec![], vec![1.0]).is_err()
        );
    }

    #[test]
    fn dukascopy_rejects_market() {
        let err = DataConfig::new(
            DataSource::Dukascopy,
            Some(Market::Spot),
            "/data",
            "EURUSD",
            "D1",
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("market"));
    }
}
