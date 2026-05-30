use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use polars::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::instrument;

/// 市场类型
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    type Err = ParseMarketTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "futures" => Ok(MarketType::Futures),
            "spot" => Ok(MarketType::Spot),
            other => Err(ParseMarketTypeError(other.to_owned())),
        }
    }
}

/// 市场类型解析错误
#[derive(Debug, Clone, Error)]
#[error("unknown market type: {0}")]
pub struct ParseMarketTypeError(pub String);

/// 数据加载器错误
#[derive(Debug, Error)]
pub enum DataError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Polars error: {0}")]
    Polars(#[from] PolarsError),

    #[error("directory not found: {0}")]
    NotFound(String),

    #[error("missing required column (tried: {0})")]
    MissingColumn(String),
}

/// K 线数据结构 (前端友好格式)
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

type CacheKey = (MarketType, String, String);

/// 本地 K 线数据加载器，带内存缓存。
///
/// 从 Parquet 文件加载 K 线数据，支持 futures 和 spot 两种市场类型。
/// 目录结构:
/// - `$data_dir/futures/um/daily/klines/{pair}/{timeframe}/*.parquet`
/// - `$data_dir/spot/daily/klines/{pair}/{timeframe}/*.parquet`
///
/// 使用 `moka` 内存缓存避免重复读取磁盘，缓存 TTL 5 分钟。
pub struct DataLoader {
    data_dir: PathBuf,
    klines_cache: moka::sync::Cache<CacheKey, Arc<Vec<Kline>>>,
}

impl DataLoader {
    /// 创建新的数据加载器。
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            klines_cache: moka::sync::Cache::builder()
                .max_capacity(200)
                .time_to_live(Duration::from_secs(300))
                .build(),
        }
    }

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

    /// 获取指定市场的所有交易对。
    pub fn list_pairs(&self, market_type: MarketType) -> Result<Vec<String>, DataError> {
        let base = self.base_path(market_type);
        if !base.exists() {
            return Ok(vec![]);
        }
        let mut pairs: Vec<String> = std::fs::read_dir(&base)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        pairs.sort();
        Ok(pairs)
    }

    /// 获取指定交易对的所有时间周期（仅包含有 parquet 文件的目录）。
    pub fn list_timeframes(
        &self,
        market_type: MarketType,
        pair: &str,
    ) -> Result<Vec<String>, DataError> {
        let pair_dir = self.base_path(market_type).join(pair);
        if !pair_dir.exists() {
            return Ok(vec![]);
        }
        let mut timeframes: Vec<String> = std::fs::read_dir(&pair_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| has_parquet_files(&e.path()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        timeframes.sort_by_key(|a| timeframe_order(a));
        Ok(timeframes)
    }

    /// 加载 K 线数据，支持 `before` / `after` 时间戳过滤和 `limit` 条数限制。
    ///
    /// `limit` 取倒数 N 条（即时间上最新的 N 条）。结果按时间升序排列。
    pub fn load_klines_filtered(
        &self,
        market_type: MarketType,
        pair: &str,
        timeframe: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: Option<usize>,
    ) -> Result<Vec<Kline>, DataError> {
        let klines = self.load_all_klines(market_type, pair, timeframe)?;

        let iter = klines
            .iter()
            .filter(|k| before.is_none_or(|t| k.time < t))
            .filter(|k| after.is_none_or(|t| k.time > t));

        let result: Vec<Kline> = match limit {
            Some(n) => iter
                .rev()
                .take(n)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            None => iter.cloned().collect(),
        };

        Ok(result)
    }

    #[instrument(skip(self))]
    fn load_all_klines(
        &self,
        market_type: MarketType,
        pair: &str,
        timeframe: &str,
    ) -> Result<Arc<Vec<Kline>>, DataError> {
        let key = (market_type, pair.to_owned(), timeframe.to_owned());
        if let Some(cached) = self.klines_cache.get(&key) {
            tracing::debug!(?key, "cache hit");
            return Ok(cached);
        }
        tracing::debug!(?key, "cache miss — reading parquet files");

        let tf_dir = self.base_path(market_type).join(pair).join(timeframe);
        if !tf_dir.exists() {
            tracing::warn!(path = %tf_dir.display(), "kline directory not found");
            return Err(DataError::NotFound(format!("{:?}", tf_dir)));
        }

        let files = list_parquet_files(&tf_dir)?;
        if files.is_empty() {
            tracing::warn!(path = %tf_dir.display(), "no parquet files");
            return Err(DataError::NotFound(format!(
                "no parquet files in {:?}",
                tf_dir
            )));
        }

        let klines = read_parquet_files(&files)?;
        let result = Arc::new(klines);
        self.klines_cache.insert(key, result.clone());
        Ok(result)
    }

    /// 清空内存缓存。
    pub fn invalidate_cache(&self) {
        self.klines_cache.invalidate_all();
    }
}

/// Check if a directory contains at least one `.parquet` file.
fn has_parquet_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("parquet"))
        })
        .unwrap_or(false)
}

/// List all `.parquet` files in a directory, sorted by name.
fn list_parquet_files(dir: &Path) -> Result<Vec<PathBuf>, DataError> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("parquet"))
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
}

/// Try to find a column by any of the given names and return it as `Int64Chunked`.
fn get_i64_col(df: &DataFrame, names: &[&str]) -> Result<Int64Chunked, DataError> {
    for name in names {
        if let Ok(col) = df.column(name)
            && let Ok(chunked) = col.i64()
        {
            return Ok(chunked.clone());
        }
    }
    Err(DataError::MissingColumn(names.join("/")))
}

/// Try to find a column by any of the given names and return it as `Float64Chunked`.
fn get_f64_col(df: &DataFrame, names: &[&str]) -> Result<Float64Chunked, DataError> {
    for name in names {
        if let Ok(col) = df.column(name)
            && let Ok(chunked) = col.f64()
        {
            return Ok(chunked.clone());
        }
    }
    Err(DataError::MissingColumn(names.join("/")))
}

/// Read and merge multiple parquet files into a single sorted `Vec<Kline>`.
///
/// Uses polars lazy scanning for efficient predicate pushdown and diagonal
/// concatenation for files with different column sets.
#[instrument(skip(files))]
fn read_parquet_files(files: &[PathBuf]) -> Result<Vec<Kline>, DataError> {
    tracing::debug!(count = files.len(), "reading parquet files");

    let lfs: Vec<LazyFrame> = files
        .iter()
        .map(|path| {
            LazyFrame::scan_parquet(path, ScanArgsParquet::default()).map_err(DataError::Polars)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let df = concat(
        &lfs,
        UnionArgs {
            diagonal: true,
            parallel: true,
            ..Default::default()
        },
    )
    .map_err(DataError::Polars)?
    .sort(
        ["open_time"],
        SortMultipleOptions::new().with_order_descending(false),
    )
    .collect()
    .map_err(DataError::Polars)?;

    if df.height() == 0 {
        return Ok(vec![]);
    }

    tracing::debug!(rows = df.height(), "parquet data loaded");

    let ts = get_i64_col(&df, &["open_time", "time", "timestamp"])?;
    let open = get_f64_col(&df, &["open"])?;
    let high = get_f64_col(&df, &["high"])?;
    let low = get_f64_col(&df, &["low"])?;
    let close = get_f64_col(&df, &["close"])?;
    let volume = get_f64_col(&df, &["volume", "vol"])?;

    let mut klines = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let (Some(o), Some(h), Some(l), Some(c)) =
            (open.get(i), high.get(i), low.get(i), close.get(i))
        else {
            continue;
        };
        if o.is_nan() || h.is_nan() || l.is_nan() || c.is_nan() {
            continue;
        }
        let t = ts.get(i).unwrap_or(0);
        let v = volume.get(i).unwrap_or(0.0);
        klines.push(Kline {
            time: t,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: v,
        });
    }

    klines.dedup_by_key(|k| k.time);
    tracing::debug!(count = klines.len(), "parquet rows after dedup");
    Ok(klines)
}

/// Map a timeframe string to a numeric ordering key (lower = earlier).
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
    use tempfile::TempDir;

    fn write_test_parquet(
        dir: &Path,
        market: MarketType,
        pair: &str,
        timeframe: &str,
        rows: &[(i64, f64, f64, f64, f64, f64)],
    ) {
        let tf_dir = match market {
            MarketType::Futures => dir
                .join("futures")
                .join("um")
                .join("daily")
                .join("klines")
                .join(pair)
                .join(timeframe),
            MarketType::Spot => dir
                .join("spot")
                .join("daily")
                .join("klines")
                .join(pair)
                .join(timeframe),
        };
        std::fs::create_dir_all(&tf_dir).unwrap();

        let times: Vec<i64> = rows.iter().map(|r| r.0).collect();
        let opens: Vec<f64> = rows.iter().map(|r| r.1).collect();
        let highs: Vec<f64> = rows.iter().map(|r| r.2).collect();
        let lows: Vec<f64> = rows.iter().map(|r| r.3).collect();
        let closes: Vec<f64> = rows.iter().map(|r| r.4).collect();
        let volumes: Vec<f64> = rows.iter().map(|r| r.5).collect();

        let mut df = df!(
            "open_time" => &times,
            "open" => &opens,
            "high" => &highs,
            "low" => &lows,
            "close" => &closes,
            "volume" => &volumes,
        )
        .unwrap();

        let path = tf_dir.join(format!("{pair}-{timeframe}-2024-01-01.parquet"));
        let mut file = std::fs::File::create(path).unwrap();
        ParquetWriter::new(&mut file).finish(&mut df).unwrap();
    }

    fn test_rows() -> Vec<(i64, f64, f64, f64, f64, f64)> {
        vec![
            (1_000_000, 100.0, 105.0, 95.0, 102.0, 10.0),
            (2_000_000, 102.0, 107.0, 97.0, 104.0, 20.0),
            (3_000_000, 104.0, 109.0, 99.0, 106.0, 30.0),
        ]
    }

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
    fn test_parse_market_type_error_display() {
        let e = ParseMarketTypeError("bad".into());
        assert_eq!(e.to_string(), "unknown market type: bad");
    }

    #[test]
    fn test_timeframe_order() {
        assert!(timeframe_order("1m") < timeframe_order("5m"));
        assert!(timeframe_order("1h") < timeframe_order("4h"));
        assert!(timeframe_order("4h") < timeframe_order("1d"));
        assert_eq!(timeframe_order("unknown"), 99);
    }

    #[test]
    fn test_list_pairs_empty_dir() {
        let dir = TempDir::new().unwrap();
        let loader = DataLoader::new(dir.path().to_path_buf());
        let pairs = loader.list_pairs(MarketType::Futures).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_list_pairs_with_data() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "ETHUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let pairs = loader.list_pairs(MarketType::Futures).unwrap();
        assert_eq!(pairs, vec!["BTCUSDT", "ETHUSDT"]);
    }

    #[test]
    fn test_list_timeframes_only_with_parquet() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1d",
            &test_rows(),
        );
        // create empty directory without parquet
        let empty_dir = dir
            .path()
            .join("futures")
            .join("um")
            .join("daily")
            .join("klines")
            .join("BTCUSDT")
            .join("1w");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let loader = DataLoader::new(dir.path().to_path_buf());
        let tfs = loader
            .list_timeframes(MarketType::Futures, "BTCUSDT")
            .unwrap();
        assert_eq!(tfs, vec!["1h", "1d"]);
    }

    #[test]
    fn test_load_klines_filtered_all() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines.len(), 3);
    }

    #[test]
    fn test_load_klines_filtered_before() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(
                MarketType::Futures,
                "BTCUSDT",
                "1h",
                Some(2_500_000),
                None,
                None,
            )
            .unwrap();
        assert_eq!(klines.len(), 2);
    }

    #[test]
    fn test_load_klines_filtered_after() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(
                MarketType::Futures,
                "BTCUSDT",
                "1h",
                None,
                Some(1_500_000),
                None,
            )
            .unwrap();
        assert_eq!(klines.len(), 2);
    }

    #[test]
    fn test_load_klines_filtered_limit() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, Some(2))
            .unwrap();
        assert_eq!(klines.len(), 2);
        // limit takes the last N (latest)
        assert_eq!(klines[0].time, 2_000_000);
    }

    #[test]
    fn test_load_klines_skips_nan() {
        let dir = TempDir::new().unwrap();
        let rows = vec![
            (1_000_000, 100.0, 105.0, 95.0, 102.0, 10.0),
            (2_000_000, f64::NAN, 107.0, 97.0, 104.0, 20.0),
            (3_000_000, 104.0, 109.0, 99.0, 106.0, 30.0),
        ];
        write_test_parquet(dir.path(), MarketType::Futures, "BTCUSDT", "1h", &rows);

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines.len(), 2);
    }

    #[test]
    fn test_load_klines_dedup() {
        let dir = TempDir::new().unwrap();
        // Write same data across two parquet files
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines.len(), 3);
    }

    #[test]
    fn test_load_klines_cached() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());

        // First call loads from disk
        let klines1 = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines1.len(), 3);

        // Second call should hit cache (removing files wouldn't affect cached result)
        std::fs::remove_dir_all(
            dir.path()
                .join("futures")
                .join("um")
                .join("daily")
                .join("klines")
                .join("BTCUSDT"),
        )
        .unwrap();
        let klines2 = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines2.len(), 3);
    }

    #[test]
    fn test_load_klines_not_found() {
        let dir = TempDir::new().unwrap();
        let loader = DataLoader::new(dir.path().to_path_buf());
        let result =
            loader.load_klines_filtered(MarketType::Futures, "NONEXISTENT", "1h", None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalidate_cache() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        loader.invalidate_cache();

        // After invalidation, remove the data, expect error
        std::fs::remove_dir_all(
            dir.path()
                .join("futures")
                .join("um")
                .join("daily")
                .join("klines")
                .join("BTCUSDT"),
        )
        .unwrap();
        let result =
            loader.load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_cache_access() {
        let dir = TempDir::new().unwrap();
        write_test_parquet(
            dir.path(),
            MarketType::Futures,
            "BTCUSDT",
            "1h",
            &test_rows(),
        );

        let loader = Arc::new(DataLoader::new(dir.path().to_path_buf()));

        std::thread::scope(|s| {
            for _ in 0..10 {
                let loader = Arc::clone(&loader);
                s.spawn(move || {
                    let klines = loader
                        .load_klines_filtered(
                            MarketType::Futures,
                            "BTCUSDT",
                            "1h",
                            None,
                            None,
                            None,
                        )
                        .unwrap();
                    assert_eq!(klines.len(), 3);
                });
            }
        });
    }
}
