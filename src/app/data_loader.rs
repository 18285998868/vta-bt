use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use thiserror::Error;
use tracing::instrument;

use crate::utils::{has_kline_files, timeframe_order};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Error)]
#[error("unknown market type: {0}")]
pub struct ParseMarketTypeError(pub String);

#[derive(Debug, Error)]
pub enum DataError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("kline-reader error: {0}")]
    Reader(#[from] kline_reader::ReaderError),

    #[error("directory not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Kline {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

type CacheKey = (MarketType, String, String);

pub struct DataLoader {
    data_dir: PathBuf,
    klines_cache: moka::sync::Cache<CacheKey, Arc<Vec<Kline>>>,
}

impl DataLoader {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            klines_cache: moka::sync::Cache::builder()
                .max_capacity(200)
                .time_to_live(Duration::from_secs(300))
                .build(),
        }
    }

    /// Futures 复用 kline-reader 的 binance 布局
    /// (`{dir}/binance/data/um/daily/klines/{symbol}/{tf}/*.zip`)；
    /// Spot 暂无对应数据源（kline-reader 仅 binance），返回 `None`，调用方按空/404 处理。
    fn base_path(&self, market_type: MarketType) -> Option<PathBuf> {
        match market_type {
            MarketType::Futures => Some(
                self.data_dir
                    .join("binance")
                    .join("data")
                    .join("um")
                    .join("daily")
                    .join("klines"),
            ),
            MarketType::Spot => None,
        }
    }

    pub fn list_pairs(&self, market_type: MarketType) -> Result<Vec<String>, DataError> {
        let Some(base) = self.base_path(market_type) else {
            return Ok(vec![]);
        };
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

    pub fn list_timeframes(
        &self,
        market_type: MarketType,
        pair: &str,
    ) -> Result<Vec<String>, DataError> {
        let Some(base) = self.base_path(market_type) else {
            return Ok(vec![]);
        };
        let pair_dir = base.join(pair);
        if !pair_dir.exists() {
            return Ok(vec![]);
        }
        let mut timeframes: Vec<String> = std::fs::read_dir(&pair_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| has_kline_files(&e.path()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        timeframes.sort_by_key(|a| timeframe_order(a));
        Ok(timeframes)
    }

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

        let mut result: Vec<Kline> = iter.cloned().collect();
        if let Some(n) = limit {
            let start = result.len().saturating_sub(n);
            result = result.split_off(start);
        }

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
        tracing::debug!(?key, "cache miss — reading kline files");

        let Some(tf_dir) = self
            .base_path(market_type)
            .map(|base| base.join(pair).join(timeframe))
        else {
            tracing::warn!("market not backed by kline-reader yet");
            return Err(DataError::NotFound(format!(
                "market not supported: {pair}/{timeframe}"
            )));
        };
        if !tf_dir.exists() {
            tracing::warn!(path = %tf_dir.display(), "kline directory not found");
            return Err(DataError::NotFound(format!("{:?}", tf_dir)));
        }

        if !has_kline_files(&tf_dir) {
            tracing::warn!(path = %tf_dir.display(), "no kline zip files");
            return Err(DataError::NotFound(format!(
                "no kline zip files in {:?}",
                tf_dir
            )));
        }

        let cfg = kline_reader::DataConfig::new(
            kline_reader::DataSource::Binance,
            &self.data_dir,
            pair,
            timeframe,
            None,
            None,
        )?;
        let cols = kline_reader::load_columns(&cfg)?;
        if cols.is_empty() {
            return Err(DataError::NotFound(format!(
                "no klines loaded for {pair}/{timeframe}"
            )));
        }
        if cols.len() > 500_000 {
            tracing::warn!(
                rows = cols.len(),
                "large kline dataset loaded — consider using before/after filters to reduce memory"
            );
        }

        let mut klines = Vec::with_capacity(cols.len());
        for i in 0..cols.len() {
            let (o, h, l, c, v) = (
                cols.open()[i],
                cols.high()[i],
                cols.low()[i],
                cols.close()[i],
                cols.volume()[i],
            );
            if o.is_nan() || h.is_nan() || l.is_nan() || c.is_nan() || v.is_nan() {
                continue;
            }
            klines.push(Kline {
                time: cols.time()[i],
                open: o,
                high: h,
                low: l,
                close: c,
                volume: v,
            });
        }

        klines.dedup_by_key(|k| k.time);
        tracing::debug!(count = klines.len(), "klines after dedup");
        let result = Arc::new(klines);
        self.klines_cache.insert(key, result.clone());
        Ok(result)
    }

    pub fn invalidate_cache(&self) {
        self.klines_cache.invalidate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn binance_tf_dir(dir: &Path, pair: &str, timeframe: &str) -> PathBuf {
        dir.join("binance")
            .join("data")
            .join("um")
            .join("daily")
            .join("klines")
            .join(pair)
            .join(timeframe)
    }

    fn write_test_zip(
        dir: &Path,
        pair: &str,
        timeframe: &str,
        name: &str,
        rows: &[(i64, f64, f64, f64, f64, f64)],
    ) {
        let tf_dir = binance_tf_dir(dir, pair, timeframe);
        std::fs::create_dir_all(&tf_dir).unwrap();
        let mut csv = String::new();
        for r in rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},0,0,0,0,0,0\n",
                r.0, r.1, r.2, r.3, r.4, r.5
            ));
        }
        let zip_path = tf_dir.join(name);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let csv_name = name.replace(".zip", ".csv");
        zip.start_file::<&str, ()>(&csv_name, zip::write::FileOptions::default())
            .unwrap();
        use std::io::Write as _;
        zip.write_all(csv.as_bytes()).unwrap();
        zip.finish().unwrap();
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
    fn test_list_pairs_empty_dir() {
        let dir = TempDir::new().unwrap();
        let loader = DataLoader::new(dir.path().to_path_buf());
        let pairs = loader.list_pairs(MarketType::Futures).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_list_pairs_with_data() {
        let dir = TempDir::new().unwrap();
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );
        write_test_zip(
            dir.path(),
            "ETHUSDT",
            "1h",
            "ETHUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let pairs = loader.list_pairs(MarketType::Futures).unwrap();
        assert_eq!(pairs, vec!["BTCUSDT", "ETHUSDT"]);
    }

    #[test]
    fn test_list_timeframes_only_with_zip() {
        let dir = TempDir::new().unwrap();
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1d",
            "BTCUSDT-1d-2024-01-01.zip",
            &test_rows(),
        );
        let empty_dir = binance_tf_dir(dir.path(), "BTCUSDT", "1w");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let loader = DataLoader::new(dir.path().to_path_buf());
        let tfs = loader
            .list_timeframes(MarketType::Futures, "BTCUSDT")
            .unwrap();
        assert_eq!(tfs, vec!["1h", "1d"]);
    }

    #[test]
    fn test_spot_not_supported_yet() {
        let dir = TempDir::new().unwrap();
        let loader = DataLoader::new(dir.path().to_path_buf());
        assert!(loader.list_pairs(MarketType::Spot).unwrap().is_empty());
        assert!(
            loader
                .load_klines_filtered(MarketType::Spot, "BTCUSDT", "1h", None, None, None)
                .is_err()
        );
    }

    #[test]
    fn test_load_klines_filtered_all() {
        let dir = TempDir::new().unwrap();
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, Some(2))
            .unwrap();
        assert_eq!(klines.len(), 2);
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &rows,
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        let klines = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines.len(), 2);
    }

    #[test]
    fn test_load_klines_dedup() {
        let dir = TempDir::new().unwrap();
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-02.zip",
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());

        let klines1 = loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        assert_eq!(klines1.len(), 3);

        std::fs::remove_dir_all(binance_tf_dir(dir.path(), "BTCUSDT", "1h")).unwrap();
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
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
            &test_rows(),
        );

        let loader = DataLoader::new(dir.path().to_path_buf());
        loader
            .load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None)
            .unwrap();
        loader.invalidate_cache();

        std::fs::remove_dir_all(binance_tf_dir(dir.path(), "BTCUSDT", "1h")).unwrap();
        let result =
            loader.load_klines_filtered(MarketType::Futures, "BTCUSDT", "1h", None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_cache_access() {
        let dir = TempDir::new().unwrap();
        write_test_zip(
            dir.path(),
            "BTCUSDT",
            "1h",
            "BTCUSDT-1h-2024-01-01.zip",
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
