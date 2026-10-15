# tradingview

## 项目目标

> TradingView K 线数据本地 HTTP 服务：读本地 parquet（futures/spot），经 REST 提供给前端图表。
>
> - 后端（`tradingview-backend`）：axum 服务，`GET /api/health|market-types|pairs|timeframes|klines`，polars 读 parquet + moka 缓存，前端静态目录同服。
> - 前端（`tradingview-frontend`）：vite + lightweight-charts K 线图，dev 经 proxy 调 `/api`。

## 技术选型

- rust（后端）/ TypeScript（前端）
- edition = "2024"
- HTTP：axum 0.8 + tower-http 0.7（cors/fs）
- 数据：polars 0.55（parquet/lazy）+ moka 0.12（sync/future 缓存）
- 前端：vite 6 + lightweight-charts 4 + technicalindicators 3
- 配置：固定路径 `config/config.toml`（无 CLI/env 覆盖）+ serde（derive）
- 错误：thiserror 2（`AppError`）+ anyhow 1（入口收敛）

## 冒烟配置

```toml
# config/config.toml（固定路径）
host = "127.0.0.1" # 可选，默认 127.0.0.1
port = 3001 # 可选，默认 3001
data_dir = "/data/nutilustrader/data" # 必填，无默认，须指向 parquet 根
# frontend_dir = "<workspace>/frontend/dist" # 可选，默认 CARGO_MANIFEST_DIR/frontend/dist
```

## 逻辑编排

```
启动 -> main.rs: dotenv → tracing_subscriber::fmt(tradingview_backend=info)
  → Config::load()（固定 config/config.toml）→ run(&config)
v
run(&config):
  1. data_dir 不存在直接 AppError::Config；frontend dist 缺失只 warn（纯 API 模式）
  2. AppState::new(data_dir)（DataLoader + moka cache）
  3. create_router() → /api/* 路由 + CORS 全放行（warn 提示生产收紧）+ ServeDir(frontend/dist)
  4. TcpListener::bind(host:port) + axum::serve 常驻
v
请求 -> Router -> Handler(Query+State) -> DataLoader（MarketType futures|spot → parquet → moka）-> ApiResponse{success, data} Json
```

## 架构设计

```
src/main.rs         # 入口：#[tokio::main] → dotenv → tracing 初始化 → Config::load() → run(&config)；anyhow 收敛
src/lib.rs          # 只做导出 + run(config)：config/data_loader/error/routes 模块声明；CORS 全放行（warn）；data_dir 校验；ServeDir 挂载；serve
src/config/mod.rs   # 支撑件：pub mod default + pub use Config
src/config/default.rs # 支撑件：Config{host/port/data_dir/frontend_dir} serde 默认 + load() 读固定 config/config.toml
src/error.rs        # 支撑件：AppError(Io/Config/Data/Parse) + From<AddrParseError/anyhow>
src/routes.rs       # 执行代码：AppState + ApiResponse + create_router（/api/health|market-types|pairs|timeframes|klines，klines/pairs/timeframes 带防目录穿越单测）
src/data_loader.rs  # 执行代码：MarketType{Futures,Spot}(Display/FromStr) + DataLoader::new(data_dir)，polars 读 parquet，moka 缓存
frontend/           # 前端工程：src/{api,app,chart,indicators,main,types} + vite.config(dev 5173, proxy /api→3001) → dist/ 产物被后端同服
```

## 实现

### main.rs / lib.rs

- `main` 只做三件事：日志初始化 → `Config::load()` → `run(&config)`。
- `run` 先校验 `data_dir.is_dir()`（否则直接 `Config` 错）；`frontend_dir` 缺失降级纯 API 模式（warn 不中断）。

### config

解析层：

1. 固定路径 `config/config.toml`，无 CLI 参数覆盖。
2. TOML 可解析，失败带路径。

语义层：`host/port/frontend_dir` 有缺省；`data_dir` 必填（示例指向 parquet 根）。

### routes & data_loader

- `GET /api/health` 健康检查；`/api/market-types|pairs|timeframes|klines` 按 `market_type` + `pair` + `timeframe` 取数。
- 入参做目录穿越防护（`..`/`/etc/passwd` 类输入有单测覆盖）。
- `DataLoader` 按市场类型定位 parquet，读后进 moka 缓存，统一包 `ApiResponse{success, data}`。

## 问题

- 当 `data_dir` 指向不存在目录时，
  - 解决：启动即校验，`AppError::Config` fast-fail
  - 文件：`src/lib.rs`
- 当只想要 API 不要前端时，
  - 解决：`frontend_dir` 缺失仅 warn，服务以降级纯 API 模式继续
  - 文件：`src/lib.rs`
- 当生产环境需要收紧跨域时，
  - 解决：当前 CORS 全放行并打 warn，按需改 `setup_cors` 指定 origin
  - 文件：`src/lib.rs`
- 当请求试图穿越目录时，
  - 解决：入参校验，单测覆盖 `..` 与绝对路径攻击串
  - 文件：`src/routes.rs`

## 完整架构

```
src/main.rs         # 入口：#[tokio::main] dotenv → tracing → Config::load → run
src/lib.rs          # pub mod config/data_loader/error/routes + run(config)：校验/状态/路由/CORS/静态/serve
src/config/mod.rs   # pub mod default + pub use Config
src/config/default.rs # Config{host/port/data_dir/frontend_dir} + load(固定路径)
src/error.rs        # AppError(Io/Config/Data/Parse)
src/routes.rs       # AppState/DataLoader 持有 + ApiResponse + 5 个 GET 路由 + 穿越防护
src/data_loader.rs  # MarketType + DataLoader：polars parquet + moka
frontend/src/       # api/app/chart/indicators/main/types + style.css + vite-env
```

## 完整配置

```toml
# config/config.toml（固定路径，无 CLI 覆盖）
host = "127.0.0.1" # 可选，默认 127.0.0.1，监听地址
port = 3001 # 可选，默认 3001
data_dir = "/data/nutilustrader/data" # 必填，无默认，parquet 根
frontend_dir = "/data/nutilustrader/creats/tradingview/frontend/dist" # 可选，默认 <workspace>/frontend/dist
```

- 全量示例见 `config/config.example.toml`。

## 完整执行逻辑

- src/main.rs
  - 入口
  - `dotenv().ok()` → `tracing_subscriber::fmt`（`tradingview_backend=info` 缺省）→ `Config::load()` → `run(&config)`
- src/lib.rs
  - `run(&Config)` 唯一编排：校验 `data_dir` → 建 `AppState` → 路由+CORS+静态 → `bind+serve`
- src/config/default.rs
  - `load()`：读固定 `config/config.toml` → `toml::from_str` → 缺省补齐
- src/error.rs
  - `AppError`（thiserror）：Io/Config/Data/Parse
- src/routes.rs
  - 5 个 GET handler：health / market-types / pairs / timeframes / klines
- src/data_loader.rs
  - `MarketType` 解析 → parquet 定位 → polars 读 → moka 存 → 返回行集
- frontend/
  - `api.ts` 调 `/api/*` → `chart.ts`（lightweight-charts）渲染 → `indicators.ts`（technicalindicators）叠加

## 命令

```bash
cargo check -p tradingview-backend
cargo clippy -p tradingview-backend --all-targets
cargo test -p tradingview-backend
cargo fmt
cargo run -p tradingview-backend                  # 读 config/config.toml，:3001 对外
cd frontend && npm install && npm run dev         # 前端 dev（:5173，/api 代理到 :3001）
cd frontend && npm run build                       # 产物进 frontend/dist，被后端同服
```

## 配置

见 `config/config.example.toml`（全字段）与 `config/config.toml`（当前生效）。
