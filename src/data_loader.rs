//! 本地K线数据加载器
//!
//! 从 Parquet 文件加载 K 线数据，支持 futures 和 spot 两种市场类型
//! 目录结构:
//! $data_dir/futures/um/daily/klines/{pair}/{timeframe}/*.parquet
//! $data_dir/spot/daily/klines/{pair}/{timeframe}/*.parquet

use std::fs::File;
use std::path::{Path, PathBuf};

use arrow::array::{Float64Array, Int64Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};

/// 市场类型
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketType {
    #[default]
    Futures,
    Spot,
}

impl std::fmt::Display for MarketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketType::Futures => write!(f, "futures"),
            MarketType::Spot => write!(f, "spot"),
        }
    }
}

impl std::str::FromStr for MarketType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "futures" => Ok(MarketType::Futures),
            "spot" => Ok(MarketType::Spot),
            _ => Err(format!("Unknown market type: {}", s)),
        }
    }
}

/// K线数据结构 (前端友好格式)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kline {
    /// 开盘时间 (毫秒时间戳)
    pub time: i64,
    /// 开盘价
    pub open: f64,
    /// 最高价
    pub high: f64,
    /// 最低价
    pub low: f64,
    /// 收盘价
    pub close: f64,
    /// 成交量
    pub volume: f64,
}

/// 数据加载器
pub struct DataLoader {
    data_dir: PathBuf,
}

impl DataLoader {
    /// 创建新的数据加载器
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// 获取基础目录
    fn base_path(&self, market_type: MarketType) -> PathBuf {
        match market_type {
            MarketType::Futures => self
                .data_dir
                .join("futures")
                .join("um")
                .join("daily")
                .join("klines"),
            MarketType::Spot => self.data_dir.join("spot").join("daily").join("klines"),
        }
    }

    /// 获取所有交易对
    pub fn list_pairs(&self, market_type: MarketType) -> anyhow::Result<Vec<String>> {
        let base = self.base_path(market_type);
        if !base.exists() {
            return Ok(vec![]);
        }

        let mut pairs = Vec::new();
        for entry in std::fs::read_dir(&base)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name()
            {
                pairs.push(name.to_string_lossy().to_string());
            }
        }
        pairs.sort();
        Ok(pairs)
    }

    /// 获取指定交易对的所有时间周期
    pub fn list_timeframes(
        &self,
        market_type: MarketType,
        pair: &str,
    ) -> anyhow::Result<Vec<String>> {
        let pair_dir = self.base_path(market_type).join(pair);
        if !pair_dir.exists() {
            return Ok(vec![]);
        }

        let mut timeframes = Vec::new();
        for entry in std::fs::read_dir(&pair_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name()
            {
                let tf = name.to_string_lossy().to_string();
                let has_parquet = std::fs::read_dir(&path)?.any(|e| {
                    e.ok()
                        .and_then(|e| {
                            e.path()
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .map(|s| s == "parquet")
                        })
                        .unwrap_or(false)
                });
                if has_parquet {
                    timeframes.push(tf);
                }
            }
        }
        timeframes.sort_by_key(|a| timeframe_order(a));
        Ok(timeframes)
    }

    /// 加载K线数据，支持 before/after 过滤和 limit
    pub fn load_klines_filtered(
        &self,
        market_type: MarketType,
        pair: &str,
        timeframe: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Kline>> {
        let mut klines = self.load_all_klines(market_type, pair, timeframe)?;

        if let Some(ts) = before {
            klines.retain(|k| k.time < ts);
        }
        if let Some(ts) = after {
            klines.retain(|k| k.time > ts);
        }

        if let Some(limit) = limit
            && klines.len() > limit
        {
            let start = klines.len().saturating_sub(limit);
            klines = klines[start..].to_vec();
        }

        Ok(klines)
    }

    fn load_all_klines(
        &self,
        market_type: MarketType,
        pair: &str,
        timeframe: &str,
    ) -> anyhow::Result<Vec<Kline>> {
        let tf_dir = self.base_path(market_type).join(pair).join(timeframe);
        if !tf_dir.exists() {
            return Err(anyhow::anyhow!(
                "Timeframe directory not found: {:?}",
                tf_dir
            ));
        }

        let mut files: Vec<PathBuf> = std::fs::read_dir(&tf_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("parquet"))
            .map(|e| e.path())
            .collect();
        files.sort();

        let mut all_klines = Vec::new();
        for path in files {
            match load_klines_from_parquet(&path) {
                Ok(klines) => all_klines.extend(klines),
                Err(e) => {
                    tracing::warn!("Skipping {:?}: {}", path, e);
                }
            }
        }

        all_klines.sort_by_key(|k| k.time);
        all_klines.dedup_by_key(|k| k.time);

        Ok(all_klines)
    }
}

/// 从单个 Parquet 文件加载 K 线
fn load_klines_from_parquet(path: &Path) -> anyhow::Result<Vec<Kline>> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    let mut klines = Vec::new();

    for maybe_batch in reader {
        let batch = maybe_batch?;
        let schema = batch.schema();

        let mut open_time: Option<Vec<i64>> = None;
        let mut open: Option<Vec<f64>> = None;
        let mut high: Option<Vec<f64>> = None;
        let mut low: Option<Vec<f64>> = None;
        let mut close: Option<Vec<f64>> = None;
        let mut volume: Option<Vec<f64>> = None;

        for (i, field) in schema.fields().iter().enumerate() {
            let col = batch.column(i);
            match field.name().to_lowercase().as_str() {
                "open_time" | "time" | "timestamp" => {
                    open_time = col
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .map(|a| a.values().to_vec());
                }
                "open" => {
                    open = col
                        .as_any()
                        .downcast_ref::<Float64Array>()
                        .map(|a| a.values().to_vec());
                }
                "high" => {
                    high = col
                        .as_any()
                        .downcast_ref::<Float64Array>()
                        .map(|a| a.values().to_vec());
                }
                "low" => {
                    low = col
                        .as_any()
                        .downcast_ref::<Float64Array>()
                        .map(|a| a.values().to_vec());
                }
                "close" => {
                    close = col
                        .as_any()
                        .downcast_ref::<Float64Array>()
                        .map(|a| a.values().to_vec());
                }
                "volume" | "vol" => {
                    volume = col
                        .as_any()
                        .downcast_ref::<Float64Array>()
                        .map(|a| a.values().to_vec());
                }
                _ => {}
            }
        }

        let ts = open_time.unwrap_or_else(|| {
            tracing::warn!("Missing timestamp column in {:?}", path);
            vec![]
        });
        let opens = open.unwrap_or_default();
        let highs = high.unwrap_or_default();
        let lows = low.unwrap_or_default();
        let closes = close.unwrap_or_default();
        let vols = volume.unwrap_or_default();

        let len = ts
            .len()
            .min(opens.len())
            .min(highs.len())
            .min(lows.len())
            .min(closes.len())
            .min(vols.len());

        for i in 0..len {
            if opens[i].is_nan() || highs[i].is_nan() || lows[i].is_nan() || closes[i].is_nan() {
                continue;
            }
            klines.push(Kline {
                time: ts[i],
                open: opens[i],
                high: highs[i],
                low: lows[i],
                close: closes[i],
                volume: vols[i],
            });
        }
    }

    Ok(klines)
}

/// 时间周期排序权重
fn timeframe_order(tf: &str) -> u32 {
    match tf {
        "1s" => 1,
        "5s" => 2,
        "1m" => 10,
        "3m" => 11,
        "5m" => 12,
        "15m" => 13,
        "30m" => 14,
        "1h" => 20,
        "2h" => 21,
        "4h" => 22,
        "6h" => 23,
        "8h" => 24,
        "12h" => 25,
        "1d" => 30,
        "3d" => 31,
        "1w" => 32,
        "1M" => 33,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_type_default() {
        assert_eq!(MarketType::default(), MarketType::Futures);
    }

    #[test]
    fn test_market_type_display() {
        assert_eq!(MarketType::Futures.to_string(), "futures");
        assert_eq!(MarketType::Spot.to_string(), "spot");
    }

    #[test]
    fn test_market_type_from_str() {
        assert_eq!(
            "futures".parse::<MarketType>().unwrap(),
            MarketType::Futures
        );
        assert_eq!("spot".parse::<MarketType>().unwrap(), MarketType::Spot);
        assert!("invalid".parse::<MarketType>().is_err());
    }

    #[test]
    fn test_timeframe_order() {
        assert!(timeframe_order("1m") < timeframe_order("5m"));
        assert!(timeframe_order("1h") < timeframe_order("4h"));
        assert!(timeframe_order("4h") < timeframe_order("1d"));
    }
}
