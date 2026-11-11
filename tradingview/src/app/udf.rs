//! TradingView UDF（Universal Data Feed）协议适配。
//!
//! 端点挂在 `/udf/*`，与现有 `/api/*` 互不影响。UDF 的时间戳是**秒**，
//! 本地 K 线是**毫秒**；resolution 也需要映射到各数据源的实际周期。

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::data_loader::{DataError, MarketType};
use super::routes::AppState;
use crate::utils::valid_pair;

/// UDF 单次最多返回的 bar 数（与 `/api/klines` 上限一致）。
const MAX_UDF_BARS: usize = 10_000;
/// `/udf/search` 默认/最大返回条数。
const DEFAULT_SEARCH_LIMIT: usize = 30;
const MAX_SEARCH_LIMIT: usize = 100;

/// `/udf/history` 查询参数。
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// `BINANCE:BTCUSDT` / `DUKASCOPY:EURUSD` / 裸交易对（默认 futures）。
    pub symbol: String,
    /// UDF 周期：`1`、`5`、`60`、`240`、`1D`…
    pub resolution: String,
    /// 起始时间（秒，含）。
    pub from: Option<i64>,
    /// 结束时间（秒，含）。
    pub to: Option<i64>,
    /// 可选：最多返回多少根。
    pub countback: Option<usize>,
}

/// `/udf/symbols` 查询参数。
#[derive(Debug, Deserialize)]
pub struct SymbolQuery {
    /// UDF symbol。
    pub symbol: String,
}

/// `/udf/search` 查询参数。
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// 交易对子串（大小写不敏感）。
    pub query: Option<String>,
    /// 品种类型过滤：`crypto` / `forex`。
    #[serde(rename = "type")]
    pub symbol_type: Option<String>,
    /// 交易所过滤：`BINANCE` / `DUKASCOPY`。
    pub exchange: Option<String>,
    /// 返回条数上限。
    pub limit: Option<usize>,
}

/// 解析后的 UDF symbol。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSymbol {
    market: MarketType,
    pair: String,
}

impl ParsedSymbol {
    fn exchange(&self) -> &'static str {
        match self.market {
            MarketType::Dukascopy => "DUKASCOPY",
            MarketType::Futures | MarketType::Spot => "BINANCE",
        }
    }

    fn symbol_type(&self) -> &'static str {
        match self.market {
            MarketType::Dukascopy => "forex",
            MarketType::Futures | MarketType::Spot => "crypto",
        }
    }

    fn session(&self) -> &'static str {
        match self.market {
            MarketType::Dukascopy => "24x5",
            MarketType::Futures | MarketType::Spot => "24x7",
        }
    }

    fn description(&self) -> String {
        match self.market {
            MarketType::Dukascopy => format!("{} · Dukascopy", self.pair),
            MarketType::Futures | MarketType::Spot => format!("{} · Binance Futures", self.pair),
        }
    }

    fn full_name(&self) -> String {
        format!("{}:{}", self.exchange(), self.pair)
    }

    fn pricescale(&self) -> u64 {
        match self.market {
            MarketType::Dukascopy if self.pair.ends_with("JPY") => 1_000,
            MarketType::Dukascopy
                if self.pair.starts_with("XAU") || self.pair.starts_with("XAG") =>
            {
                100
            }
            MarketType::Dukascopy => 100_000,
            MarketType::Futures | MarketType::Spot => 100,
        }
    }
}

/// 解析 UDF symbol；裸交易对默认按 futures 处理。
fn parse_symbol(raw: &str) -> Result<ParsedSymbol, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("symbol is empty".into());
    }
    let (prefix, pair_raw) = match raw.split_once(':') {
        Some((prefix, pair)) => (prefix.trim(), pair.trim()),
        None => ("", raw),
    };
    let pair = pair_raw.to_ascii_uppercase();
    if !valid_pair(&pair) {
        return Err(format!("invalid symbol: {raw}"));
    }
    let market = match prefix.to_ascii_uppercase().as_str() {
        "" => MarketType::Futures,
        "BINANCE" | "BINANCEFUTURES" | "BINANCE_FUTURES" | "BINANCE-FUTURES" | "FUTURES" => {
            MarketType::Futures
        }
        "DUKASCOPY" | "FX" => MarketType::Dukascopy,
        other => return Err(format!("unknown exchange: {other}")),
    };
    Ok(ParsedSymbol { market, pair })
}

/// UDF resolution → 本地 timeframe。
fn resolution_to_timeframe(market: MarketType, resolution: &str) -> Result<&'static str, String> {
    let r = resolution.trim();
    // UDF 的 `1M`/`1W` 是月/周线；本地不提供，不能因为大小写不敏感而误映射成 1 分钟。
    if r == "1M" || r == "1W" {
        return Err(format!("unsupported resolution: {resolution}"));
    }
    let lower = r.to_ascii_lowercase();
    let mapped = match market {
        MarketType::Futures => match lower.as_str() {
            "1" | "1m" => "1m",
            "3" | "3m" => "3m",
            "5" | "5m" => "5m",
            "15" | "15m" => "15m",
            "30" | "30m" => "30m",
            "60" | "1h" => "1h",
            "120" | "2h" => "2h",
            "240" | "4h" => "4h",
            "360" | "6h" => "6h",
            "480" | "8h" => "8h",
            "720" | "12h" => "12h",
            "1d" | "d" | "24h" => "1d",
            _ => return Err(format!("unsupported resolution: {resolution}")),
        },
        MarketType::Dukascopy => match lower.as_str() {
            "1" | "1m" | "m1" => "M1",
            "60" | "1h" | "h1" => "H1",
            "1d" | "d" | "d1" | "24h" => "D1",
            _ => return Err(format!("unsupported resolution: {resolution}")),
        },
        MarketType::Spot => return Err("spot is not available locally".into()),
    };
    Ok(mapped)
}

/// 本地 timeframe → UDF resolution；未知周期返回 `None`。
fn timeframe_to_resolution(timeframe: &str) -> Option<&'static str> {
    Some(match timeframe.to_ascii_lowercase().as_str() {
        "1m" | "m1" => "1",
        "3m" => "3",
        "5m" => "5",
        "15m" => "15",
        "30m" => "30",
        "1h" | "h1" => "60",
        "2h" => "120",
        "4h" => "240",
        "6h" => "360",
        "8h" => "480",
        "12h" => "720",
        "1d" | "d1" => "1D",
        _ => return None,
    })
}

/// UDF resolution 的排序权重（分钟数；`1D` 最大）。
fn resolution_order(resolution: &str) -> u64 {
    if resolution.eq_ignore_ascii_case("1D") {
        1_440
    } else {
        resolution.parse::<u64>().unwrap_or(u64::MAX)
    }
}

/// 把某交易对的本地周期转成 UDF `supported_resolutions`（去重 + 排序）。
fn timeframes_to_resolutions(timeframes: &[String]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = timeframes
        .iter()
        .filter_map(|tf| timeframe_to_resolution(tf))
        .collect();
    out.sort_by_key(|r| resolution_order(r));
    out.dedup();
    out
}

/// 当前服务器时间（UDF 用秒）。
pub async fn time_handler() -> Json<Value> {
    Json(json!({ "time": chrono::Utc::now().timestamp() }))
}

/// `/udf/config`。
pub async fn config_handler() -> Json<Value> {
    Json(json!({
        "supported_resolutions": ["1", "3", "5", "15", "30", "60", "120", "240", "360", "480", "720", "1D"],
        "supports_search": true,
        "supports_time": true,
        "supports_marks": false,
        "supports_timescale_marks": false,
        "supports_group_request": false,
        "supports_news": false,
        "symbols_types": [
            { "name": "Crypto", "value": "crypto" },
            { "name": "Forex", "value": "forex" }
        ],
        "exchanges": [
            { "value": "BINANCE", "name": "Binance Futures", "desc": "Binance UM futures" },
            { "value": "DUKASCOPY", "name": "Dukascopy", "desc": "Dukascopy FX / metals" }
        ]
    }))
}

fn udf_error(msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "s": "error", "errmsg": msg.into() }))
}

fn udf_no_data() -> Json<Value> {
    Json(json!({ "s": "no_data" }))
}

fn handle_history_error(e: DataError) -> Json<Value> {
    match &e {
        DataError::NotFound(_) => udf_no_data(),
        DataError::InvalidParam(msg) => udf_error(msg.clone()),
        _ => {
            tracing::error!(error = %e, "udf history failed");
            udf_error("internal error")
        }
    }
}

/// `/udf/history`：返回 UDF 要求的 `t/o/h/l/c/v` 秒级数组。
pub async fn history_handler(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    let parsed = match parse_symbol(&query.symbol) {
        Ok(p) => p,
        Err(msg) => return (StatusCode::OK, udf_error(msg)),
    };
    let timeframe = match resolution_to_timeframe(parsed.market, &query.resolution) {
        Ok(tf) => tf,
        Err(msg) => return (StatusCode::OK, udf_error(msg)),
    };

    let from = query.from.unwrap_or(0);
    let to = query.to.unwrap_or(0);
    if from < 0 || to < 0 {
        return (StatusCode::OK, udf_error("from/to must be >= 0"));
    }
    if from > 0 && to > 0 && from > to {
        return (StatusCode::OK, udf_error("from > to"));
    }

    let after = if from > 0 {
        match from.checked_mul(1000).and_then(|v| v.checked_sub(1)) {
            Some(v) => Some(v),
            None => return (StatusCode::OK, udf_error("from is out of range")),
        }
    } else {
        None
    };
    let before = if to > 0 {
        match to.checked_mul(1000).and_then(|v| v.checked_add(1)) {
            Some(v) => Some(v),
            None => return (StatusCode::OK, udf_error("to is out of range")),
        }
    } else {
        None
    };
    let limit = Some(
        query
            .countback
            .map(|n| n.clamp(1, MAX_UDF_BARS))
            .unwrap_or(MAX_UDF_BARS),
    );

    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        let pair = parsed.pair.clone();
        move || loader.load_klines_filtered(parsed.market, &pair, timeframe, before, after, limit)
    })
    .await;

    match joined {
        Ok(Ok(klines)) => {
            if klines.is_empty() {
                return (StatusCode::OK, udf_no_data());
            }
            let mut t = Vec::with_capacity(klines.len());
            let mut o = Vec::with_capacity(klines.len());
            let mut h = Vec::with_capacity(klines.len());
            let mut l = Vec::with_capacity(klines.len());
            let mut c = Vec::with_capacity(klines.len());
            let mut v = Vec::with_capacity(klines.len());
            for k in klines {
                t.push(k.time.div_euclid(1000));
                o.push(k.open);
                h.push(k.high);
                l.push(k.low);
                c.push(k.close);
                v.push(k.volume);
            }
            (
                StatusCode::OK,
                Json(json!({
                    "s": "ok",
                    "t": t,
                    "o": o,
                    "h": h,
                    "l": l,
                    "c": c,
                    "v": v
                })),
            )
        }
        Ok(Err(e)) => (StatusCode::OK, handle_history_error(e)),
        Err(e) => {
            tracing::error!(error = %e, "udf history task failed");
            (StatusCode::OK, udf_error("internal error"))
        }
    }
}

/// `/udf/symbols`：返回 Charting Library 需要的 symbol info。
pub async fn symbols_handler(
    State(state): State<AppState>,
    Query(query): Query<SymbolQuery>,
) -> impl IntoResponse {
    let parsed = match parse_symbol(&query.symbol) {
        Ok(p) => p,
        Err(msg) => return (StatusCode::NOT_FOUND, udf_error(msg)),
    };
    let market = parsed.market;
    let pair = parsed.pair.clone();
    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        move || loader.list_timeframes(market, &pair)
    })
    .await;

    let timeframes = match joined {
        Ok(Ok(tfs)) => tfs,
        Ok(Err(e)) => {
            tracing::error!(error = %e, symbol = %query.symbol, "udf symbols failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                udf_error("internal error"),
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "udf symbols task failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                udf_error("internal error"),
            );
        }
    };
    if timeframes.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            udf_error(format!("unknown symbol: {}", query.symbol)),
        );
    }

    let resolutions = timeframes_to_resolutions(&timeframes);
    let has_intraday = resolutions.iter().any(|r| *r != "1D");
    (
        StatusCode::OK,
        Json(json!({
            "name": parsed.pair,
            "full_name": parsed.full_name(),
            "ticker": parsed.pair,
            "description": parsed.description(),
            "type": parsed.symbol_type(),
            "exchange": parsed.exchange(),
            "listed_exchange": parsed.exchange(),
            "session": parsed.session(),
            "timezone": "Etc/UTC",
            "format": "price",
            "minmov": 1,
            "pricescale": parsed.pricescale(),
            "has_intraday": has_intraday,
            "has_daily": true,
            "has_weekly_and_monthly": false,
            "has_empty_bars": false,
            "supported_resolutions": resolutions
        })),
    )
}

/// `/udf/search`：在本地数据目录里按交易对子串搜索。
pub async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let markets: Vec<MarketType> = match query.exchange.as_deref() {
        None | Some("") => MarketType::SUPPORTED.to_vec(),
        Some(exchange) => match exchange.to_ascii_uppercase().as_str() {
            "BINANCE" | "BINANCEFUTURES" | "BINANCE_FUTURES" | "BINANCE-FUTURES" | "FUTURES" => {
                vec![MarketType::Futures]
            }
            "DUKASCOPY" | "FX" => vec![MarketType::Dukascopy],
            _ => return (StatusCode::OK, Json(json!([]))),
        },
    };
    let needle = query.query.unwrap_or_default().trim().to_ascii_uppercase();
    let type_filter = query
        .symbol_type
        .as_deref()
        .map(str::to_ascii_lowercase)
        .filter(|t| !t.is_empty());
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT);

    let joined = tokio::task::spawn_blocking({
        let loader = state.data_loader.clone();
        move || {
            let mut out = Vec::new();
            for market in markets {
                for pair in loader.list_pairs(market)? {
                    if !needle.is_empty() && !pair.contains(&needle) {
                        continue;
                    }
                    let symbol_type = match market {
                        MarketType::Dukascopy => "forex",
                        MarketType::Futures | MarketType::Spot => "crypto",
                    };
                    if type_filter.as_deref().is_some_and(|t| t != symbol_type) {
                        continue;
                    }
                    let parsed = ParsedSymbol {
                        market,
                        pair: pair.clone(),
                    };
                    out.push(json!({
                        "symbol": parsed.full_name(),
                        "full_name": parsed.full_name(),
                        "description": parsed.description(),
                        "exchange": parsed.exchange(),
                        "ticker": pair,
                        "type": symbol_type
                    }));
                    if out.len() >= limit {
                        return Ok::<Vec<Value>, DataError>(out);
                    }
                }
            }
            Ok(out)
        }
    })
    .await;

    match joined {
        Ok(Ok(results)) => (StatusCode::OK, Json(Value::Array(results))),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "udf search failed");
            (StatusCode::OK, Json(json!([])))
        }
        Err(e) => {
            tracing::error!(error = %e, "udf search task failed");
            (StatusCode::OK, Json(json!([])))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_and_bare_symbols() {
        let p = parse_symbol("BINANCE:BTCUSDT").unwrap();
        assert_eq!(p.market, MarketType::Futures);
        assert_eq!(p.pair, "BTCUSDT");
        assert_eq!(p.full_name(), "BINANCE:BTCUSDT");

        let p = parse_symbol("dukascopy:eurusd").unwrap();
        assert_eq!(p.market, MarketType::Dukascopy);
        assert_eq!(p.pair, "EURUSD");
        assert_eq!(p.full_name(), "DUKASCOPY:EURUSD");

        assert!(parse_symbol("").is_err());
        assert!(parse_symbol("../etc").is_err());
        assert!(parse_symbol("NYSE:BTCUSDT").is_err());
    }

    #[test]
    fn maps_udf_resolutions() {
        assert_eq!(
            resolution_to_timeframe(MarketType::Futures, "60").unwrap(),
            "1h"
        );
        assert_eq!(
            resolution_to_timeframe(MarketType::Futures, "1D").unwrap(),
            "1d"
        );
        assert_eq!(
            resolution_to_timeframe(MarketType::Dukascopy, "1").unwrap(),
            "M1"
        );
        assert_eq!(
            resolution_to_timeframe(MarketType::Dukascopy, "1D").unwrap(),
            "D1"
        );
        assert!(resolution_to_timeframe(MarketType::Futures, "2D").is_err());
        // `1M` 是月线，不能因为大小写不敏感被当成 1 分钟。
        assert!(resolution_to_timeframe(MarketType::Futures, "1M").is_err());
        assert!(resolution_to_timeframe(MarketType::Dukascopy, "1W").is_err());
    }

    #[test]
    fn pricescale_matches_fx_quote_precision() {
        assert_eq!(
            parse_symbol("DUKASCOPY:EURUSD").unwrap().pricescale(),
            100_000
        );
        assert_eq!(
            parse_symbol("DUKASCOPY:USDJPY").unwrap().pricescale(),
            1_000
        );
        assert_eq!(parse_symbol("DUKASCOPY:XAUUSD").unwrap().pricescale(), 100);
        assert_eq!(parse_symbol("BINANCE:BTCUSDT").unwrap().pricescale(), 100);
    }

    #[test]
    fn dedupes_and_sorts_supported_resolutions() {
        let tfs = vec![
            "1d".to_string(),
            "1h".to_string(),
            "M1".to_string(),
            "1m".to_string(),
            "H1".to_string(),
        ];
        assert_eq!(timeframes_to_resolutions(&tfs), vec!["1", "60", "1D"]);
    }
}
