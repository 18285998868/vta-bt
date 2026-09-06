# tradingview-backend

TradingView K-line data local HTTP service. Reads candlestick data from local parquet files (futures/spot markets) and serves them via REST API.

## 配置

服务从工作区 `config/config.toml` 读取配置（无 CLI 参数 / 环境变量覆盖）。字段：

| 字段 | 必需 | 默认值 | 说明 |
|------|------|--------|------|
| `host` | 否 | `127.0.0.1` | 监听地址 |
| `port` | 否 | `3001` | 监听端口 |
| `data_dir` | 是 | 无 | parquet 数据根目录 |
| `frontend_dir` | 否 | `<workspace>/frontend/dist` | 前端静态资源目录 |

示例见 `config/config.example.toml`。

## 主执行流程

```
main()
 ├─ tracing_subscriber::fmt() — init structured logging
 ├─ Config::load() — read config/config.toml
 ├─ AppState::new(data_dir) — create DataLoader (moka cache)
 ├─ create_router() — register REST routes
 ├─ CorsLayer::new() — open CORS (all origins)
 ├─ ServeDir("frontend/dist") — static fallback for SPA
 └─ tokio::net::TcpListener::bind + axum::serve
      └─ accept requests ──► route handlers
```

### step 1 — API request lifecycle

```mermaid
sequenceDiagram
    participant C as Client
    participant R as Router
    participant H as Handler
    participant DL as DataLoader
    participant MC as moka Cache
    participant PF as Parquet Files

    C->>R: GET /api/klines?market_type=...&pair=...&timeframe=...
    R->>H: extract Query + AppState
    H->>DL: load_klines_filtered()
    DL->>MC: get(key=(market, pair, tf))
    alt cache hit
        MC-->>DL: Arc<Vec<Kline>>
    else cache miss
        MC-->>DL: None
        DL->>PF: list_parquet_files(tf_dir)
        PF-->>DL: Vec<PathBuf>
        DL->>PF: read_parquet_files(files)
        Note over DL,PF: polars lazy scan + diagonal concat + sort by open_time
        PF-->>DL: Vec<Kline>
        DL->>MC: insert(key, Arc<Vec<Kline>>)
    end
    DL-->>H: Result<Vec<Kline>, DataError>
    H->>H: filter by before/after/limit
    H-->>C: 200 { success, data: { klines, count, ... } }
    alt error
        H-->>C: 500 { success: false, error: "internal error" }
        Note over H: error logged via tracing::error!
    end
```

### step 2 — parquet file loading

```mermaid
flowchart LR
    A[parquet files] --> B[LazyFrame::scan_parquet]
    B --> C[concat diagonal:true]
    C --> D[sort by open_time]
    D --> E[collect]
    E --> F{height > 0?}
    F -- no --> G[return empty vec]
    F -- yes --> H[get_i64_col: open_time/time/timestamp]
    F --> I[get_f64_col: open/high/low/close/volume]
    H --> J[row loop]
    I --> J
    J --> K{OHLC all Some?}
    K -- no --> L[skip row]
    K -- yes --> M{any NaN?}
    M -- yes --> L
    M -- no --> N[push Kline]
    N --> O[dedup_by_key: time]
    O --> P[return Vec<Kline>]
    L --> J
```

### step 3 — directory layout

```text
{data_dir}/
├── futures/um/daily/klines/
│   ├── BTCUSDT/
│   │   ├── 1h/
│   │   │   ├── BTCUSDT-1h-2024-01-01.parquet
│   │   │   └── BTCUSDT-1h-2024-01-02.parquet
│   │   └── 1d/
│   │       └── BTCUSDT-1d-2024-01-01.parquet
│   └── ETHUSDT/
│       └── 1h/
│           └── ETHUSDT-1h-2024-01-01.parquet
└── spot/daily/klines/
    └── BTCUSDT/
        └── 1h/
            └── BTCUSDT-1h-2024-01-01.parquet
```

## 用法

```bash
# from workspace root
cargo run -p tradingview-backend

# 自定义端口与数据目录: 编辑 config/config.toml 后启动
# http://localhost:3001/api/klines?market_type=futures&pair=BTCUSDT&timeframe=1h
```
