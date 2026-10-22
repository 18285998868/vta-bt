# tradingview

## 项目目标

> TradingView 风格本地研究终端：读本地 binance zip（futures）与 dukascopy bi5（外汇/贵金属），经 REST 提供给前端图表，并把布局、绘图、偏好持久化到本地 SQLite。
>
> - 后端（`tradingview-backend`）：axum 服务，行情/回测 GET 路由 + `/api/layouts|drawings|preferences` 状态路由，kline-reader 读数据 + moka 缓存，前端静态目录同服。
> - 前端（`tradingview-frontend`）：vite + lightweight-charts 5 原生 pane，绘图用 lightweight-charts-drawing，dev 经 proxy 调 `/api`。
> - 状态库：SQLite（rusqlite bundled），默认 `<data_dir>/tradingview.db`，只存 UI/研究状态，不存行情。

## 技术选型

- rust（后端）/ TypeScript（前端）
- edition = "2024"，工具链由 `rust-toolchain.toml` 固定为 `channel = "stable"`（永远用最新 stable，与仓库惯例一致；本机默认的 1.88 低于依赖链要求，裸 `cargo build` 会失败）
- HTTP：axum 0.8 + tower-http 0.7（cors/fs）
- 数据：kline-reader 0.1.0（workspace path 依赖，读 binance zip + dukascopy bi5）+ moka 0.12（sync 缓存）+ vector-ta 0.3.2（workspace path 依赖，与 vta 同源，保证 overlay 与信号同一实现，见 Cargo.toml 注释）
- 回测：vta 0.1.0（workspace path 依赖）——`StrategyConfig` 反序列化 + 参数网格 + `simulate_detailed`；金额为定点 `Amount`
- 前端：vite 6 + lightweight-charts 5.2 + lightweight-charts-drawing 0.6 + technicalindicators 3
- 状态库：rusqlite 0.40（bundled SQLite）+ uuid 1（布局 ID）
- 配置：`config/config.toml` + serde（derive）
- 错误：thiserror 2（库层 `AppError`）+ anyhow 1（仅入口收敛）
- 运行时：tokio 只开 `macros / rt-multi-thread / net / fs / io-util / signal`

## 冒烟配置

```toml
# config/config.toml
host = "127.0.0.1"           # 可选，默认 127.0.0.1
port = 3001                  # 可选，默认 3001
data_dir = "/data/nutilustrader/data"   # 必填，无默认，须指向数据根
# db_path = "/data/nutilustrader/data/tradingview.db"  # 可选，默认 <data_dir>/tradingview.db
# frontend_dir = "<workspace>/frontend/dist"  # 可选，默认 CARGO_MANIFEST_DIR/frontend/dist
# cors_origins = ["http://localhost:5173"]   # 可选，默认 [] = 全放行（仅本地开发）
```

**路径与日志的优先级**（都是"显式 > 通用 > 默认"）：

| 项 | 优先级 |
|---|---|
| 配置文件 | `TRADINGVIEW_CONFIG` > `./config/config.toml`（相对 CWD） > `<crate 根>/config/config.toml`（编译期绝对路径，只在开发机有效） |
| 日志过滤 | `TRADINGVIEW_LOG` > `RUST_LOG` > `tradingview_backend=info,tower_http=info` |
| `.env` | **只读 CWD 下的 `.env`**（不向上遍历目录树，且不覆盖已有环境变量） |

> 早先用 `dotenvy::dotenv()` 会一路往上找 `.env`，仓库根的 `RUST_LOG=debug` 因此无声生效，服务自己的默认过滤规则形同虚设。

## 逻辑编排

```text
启动 -> main.rs: 读 CWD/.env → tracing_subscriber::fmt(log_filter())
  → Config::load() → run(&config)
v
run(&config):
  1. data_dir 不存在直接 AppError::Config；frontend dist 缺失只 warn（纯 API 模式）
  2. 打开 StateStore（SQLite，WAL）→ AppState::new(data_dir, state_store)
     （DataLoader + moka cache + 回测并发闸门 + 状态库）
  3. create_router() → /api/* 路由 + CORS（cors_origins 为空则全放行 + warn）+ ServeDir(frontend/dist)
  4. TcpListener::bind(host:port) + axum::serve(...).with_graceful_shutdown(SIGINT/SIGTERM)
v
请求 -> Router -> Handler(Query+State) -> spawn_blocking{ DataLoader / 回测 / SQLite } -> ApiResponse{success, data, error} Json
```

## 架构设计

```text
src/main.rs         # 入口：dotenv(CWD) → tracing → Config::load() → run(&config)；anyhow 收敛
src/lib.rs          # 只做导出 + crate 级 lint + README 内联文档
src/app/mod.rs      # 执行代码：run(config) 编排（data_dir/状态库/AppState/路由+CORS/静态/serve）+ 优雅停机
src/app/routes.rs   # 执行代码：AppState（DataLoader + 状态库 + 回测缓存 + 并发闸门）+ ApiResponse + 行情/状态路由
src/app/data_loader.rs # MarketType{Futures,Spot,Dukascopy} + KlineColumns(SoA) + DataLoader：kline-reader + moka
src/app/state.rs    # 状态库：layouts/drawings/preferences CRUD + 限额校验 + SQLite 连接
src/app/strategy.rs # 扩展缝：STRATEGIES(同时是白名单)/BacktestParams/参数网格全跑 → markers/trades/equity/overlays/summary
migrations/         # SQLite 表结构（include_str! 进二进制，启动时幂等建表）
src/config.rs       # 支撑件：Config{host/port/data_dir/db_path/frontend_dir/cors_origins} + load/load_from + validate
src/error.rs        # 支撑件：AppError(Io/Config/ConfigRead/ConfigParse/Data/State/InvalidAddr, non_exhaustive)
src/utils/mod.rs    # 支撑件：valid_pair/valid_timeframe/has_kline_files/has_data_files/timeframe_order
frontend/           # 前端工程：src/{api,app,chart,indicators,main,types} + vite.config(dev 5173, proxy /api→3001)
```

## 实现

### main.rs / lib.rs

- `main` 只做三件事：日志初始化 → `Config::load()` → `run(&config)`；错误用 `anyhow::Context` 补上下文。
- `run` 先校验 `data_dir.is_dir()`（否则直接 `Config` 错）；`frontend_dir` 缺失降级纯 API 模式（warn 不中断）。
- 库层错误统一走 `AppError`（`thiserror`），**不提供 `From<anyhow::Error>`**：`anyhow::Error` 不是 `std::error::Error`，压进枚举会丢 source 链，也会把任意失败误标成"配置错误"。

### config

解析层：

1. 路径优先级见上表；TOML 可解析，失败带路径（`AppError::ConfigParse { path, source }`）。

语义层：`host/port/frontend_dir` 有缺省；`data_dir` 必填；`db_path` 缺省为 `<data_dir>/tradingview.db`，显式给出时不能为空；`cors_origins` 缺省为空（= 全放行）。路径字段一律 `PathBuf`，不用 `String` 装路径。

### routes & data_loader

- `GET /api/health` 健康检查；`/api/market-types|pairs|timeframes|klines` 按 `market_type` + `pair` + `timeframe` 取数。
- **`/api/market-types` 只列"本地确有数据源"的类型**：`futures`（binance um）+ `dukascopy`（外汇/贵金属）。`spot` 变体仍可解析但无本地数据，不对外暴露 —— 否则前端会列出"选了就是空图"的选项。
- 入参做目录穿越防护（`..`/`/etc/passwd` 类输入有单测覆盖），并限长（pair ≤ 32、timeframe ≤ 16）。
- 请求规模防护：`limit` ≤ 10000、`strategy_toml` ≤ 4096 字节，超限返回 400。
- **周期合法性由数据源裁定**：binance 支持 `1m..1d`，dukascopy 支持 `M1/H1/D1/1m/1h/1d`。输错返回 400（`DataError::InvalidParam`），并回传该源支持的周期列表。
- `DataLoader` 按市场类型定位数据文件，读后进 moka 缓存，统一包 `ApiResponse{success, data, error}`。
- 状态路由：`/api/layouts`（列表/创建）、`/api/layouts/{id}`（读取/更新/删除）、`/api/drawings?market_type=&pair=`（读取/整组替换）、`/api/preferences`（读取/合并写入）。状态错误按输入 400、内部 500 映射，404 只用于不存在的布局。
- 缓存与回测都用 **SoA 列集** `KlineColumns`（`time/open/high/low/close/volume` 六条 `Vec`）：kline-reader 本身就是列存储，回测引擎要 `&[f64]`，中间层再存 `Vec<Kline>` 会让每次回测多出 6 份结构体拷贝。
- 时间过滤用二分（`partition_point`）定位窗口，只拷贝窗口内的 bar；`limit` 取窗口内**最后** N 根。

### 并发模型（阻塞 IO 不占 worker）

tokio 的 worker 线程只做调度。读 zip（实测 52ms）与跑回测（实测 134ms/档）这类同步重活必须挪到 blocking 线程池，否则并发几个请求就能占满 12 个 worker，轻请求（`/api/health`）也一起卡死。

- `/api/pairs`、`/api/timeframes`、`/api/klines`：`spawn_blocking` 包裹数据层调用。
- `/api/strategy`：`spawn_blocking` 包裹整个回测，并经**并发闸门**（`Semaphore`，默认 4）限流 —— 每个回测持有一份 59k×6 的列 + 信号 + 资金曲线，不限流会被单个请求吃光内存。
- `/api/layouts|drawings|preferences`：所有 SQLite 操作同样走 `spawn_blocking`；连接由 `Mutex<Connection>` 串行化（本地单用户，写少读多）。
- blocking 线程里的 panic 会被 tokio 捕获成 `JoinError`，统一收敛成 500，不会变成进程级故障。

### strategy

- 入参：TOML 片段 + 资金/费率 + 时间窗。
- 两道闸门：**先算术预检**（网格档数 ≤ 16，`step = 0` / 区间反向 / `start < 1` 直接拒），**再语义校验**（复用 vta 的 `period >= 1`、`sigma ∈ [0.1, 10]`）。顺序不能反 —— vta 的 `validate` 内部会展开网格。
- **参数网格跑满**：用 vta 的 `generate_signals_batch`（rayon 并行）逐档算信号、逐档回测，每档独立；某档不可计算（如数据不足）记入 `param_grid.skipped`，不影响其余档。这与 vta 离线全网格口径一致 —— 早先只跑 `param_idx = 0`（网格首档）且响应里毫无提示。
- 响应里 `markers/trades/equity/overlays/summary` 仍是**第 0 档**（前端零改动），另加 `param_grid`（请求/实跑/跳过）与 `runs`（各档汇总，便于横向对比）。
- 网格上限 16 = **单请求计算预算**（每档 59k 根 ≈ 134ms；16 档 ≈ 2s）。
- 白名单与 `/api/strategies` 下拉列表共用同一份 `STRATEGIES` 常量，避免两份拷贝漂移。
- **金额是定点数**：vta 侧金额用 `Amount`（定点十进制），回传前端时转回 `f64`。
- 输出 DTO 的 `side` / `kind` 是枚举（`TradeSide` / `MarkerKind`），序列化结果仍是 `"long"/"short"/"entry"/"exit"`，前端契约不变。
- **指标可为 `null`**：NaN/Inf 是真实语义（零波动 ⇒ 夏普无定义；无亏损 ⇒ 盈利因子无定义），归零会让"真的是 0"与"算不出来"无法区分。前端显示 `n/a`。

### 回放（Replay）

- 前端维护两份数据：`allKlines` 是本次回放加载的完整窗口（当前取数上限 2000 根，以进入回放时的最后一根为终点），图表只接收 `allKlines[0..=cursor]` 的前缀。这样指标的输入始终截止当前游标，不会提前看到未来 bar。
- 进入回放时以**当前视口左端**为起点；若该时间落在窗口内，就从对应 bar 开始。重置回到同一游标，退出回放恢复完整窗口和进入前的视口。
- 控件：播放/暂停、单步、0.5×/1×/2×/4×/8×、重置、退出。播放到最后一根自动暂停。
- 图表层 `updateData()` 只替换可见前缀，指标/策略在结构不变时复用已有 series 与 pane（仅 `setData`），避免每根 bar 拆重建副图。
- 策略回放请求带 `before = 下一根 bar 时间`（开区间，等价于“只回测到当前 bar”）；请求按 300ms 节流、只接受最后一次结果，避免高速播放时旧结果覆盖新游标。
- 回放游标属于临时交互状态，不写入 SQLite；`last_session` 仍只保存交易对/周期/指标/策略/视口。

### 指标 parity

- 26 个指标以 Pine/TradingView 的“首有效 bar + null 暖机”为口径；未暖机位置一律 `null`，不会画成 0。
- 修正了对齐偏差：PSAR、Ichimoku、Chandelier、ADX、MFI、KST、TRIX、OBV、AO，以及 MACD/Stoch 的暖机段。
- Ichimoku 改为自实现：转换线/基准线画在当前 bar，先行 A/B 右移 `displacement` 根；不再直接采用库的“四线同起点”输出。
- 回归测试：`cd frontend && npm test`。覆盖首有效下标、无 `undefined`、暖机后无空洞、MACD 三线同轴、Ichimoku 位移、振荡指标值域，以及“完整序列 vs 回放前缀”在重叠区间逐点一致。

### UDF 数据源

- 独立的 `/udf/*` 命名空间，复用本地 `DataLoader`，不改变 `/api/*` 契约。Charting Library 的 Datafeed URL 填 `http://127.0.0.1:3011/udf` 即可。
- 端点：`GET /udf/config`、`/udf/time`、`/udf/symbols?symbol=`、`/udf/search?query=&type=&exchange=&limit=`、`/udf/history?symbol=&resolution=&from=&to=&countback=`。
- symbol：`BINANCE:BTCUSDT`（binance UM futures）或 `DUKASCOPY:EURUSD`；裸交易对默认按 futures 处理。`Spot` 本地无数据，不对外暴露。
- resolution：UDF 分钟数映射到本地周期，例如 `1→1m`、`60→1h`、`240→4h`、`1D→1d`；dukascopy 对应 `1→M1`、`60→H1`、`1D→D1`。`/udf/symbols` 会按该交易对实际存在的周期返回 `supported_resolutions`。
- `/udf/history` 的 `from/to` 是**秒级、闭区间**；返回 `{s:"ok",t,o,h,l,c,v}`，其中 `t` 也是秒。无数据返回 `{s:"no_data"}`，非法 symbol/resolution 返回 `{s:"error",errmsg}`。
- 单次最多 10000 根；`countback` 用于只要最近 N 根（`from/to` 可为 0）。时间过滤仍在 blocking 线程池执行，读盘结果走现有 moka 缓存。
- 回归测试：`cargo test -p tradingview-backend --test udf_test`，覆盖五个端点、秒级时间、闭区间边界、`no_data` 与非法 resolution。

状态数据与行情文件分离：行情仍然只读 zip/bi5，SQLite 只存研究终端的 UI 状态。默认路径 `<data_dir>/tradingview.db`，启动时幂等执行 `migrations/001_init.sql`，并开启 WAL。

- `layouts`：命名布局，一行一份**完整会话快照**（`market_type/pair/timeframe/indicators/strategy{TOML,applied}/viewport`）。名称唯一，列表只回摘要，读取详情才回 `payload`。
- `drawings`：按 `(market_type, pair)` 一行存整组绘图（`lightweight-charts-drawing` 导出的 JSON 数组）。跨周期共享；`PUT` 是整组替换，不是增量合并。
- `preferences`：KV 合并写入。`last_session` 存最近一次会话，用于启动恢复。
- 限额：布局 payload ≤ 1 MiB；绘图 ≤ 500 个/对、单图 ≤ 512 锚点、整组 ≤ 1 MiB；偏好 ≤ 128 键 / 256 KiB；HTTP body ≤ 2 MiB。
- 前端自动保存：会话 800ms debounce，绘图 600ms debounce；恢复/加载布局期间用 `suppressSave` 抑制回写，绘图在服务端加载完成前用 `drawingsLoaded` 阻止空画布覆盖。

## 问题

- 当 `data_dir` 指向不存在目录时，
  - 解决：启动即校验，`AppError::Config` fast-fail
  - 文件：`src/app/mod.rs`
- 当只想要 API 不要前端时，
  - 解决：`frontend_dir` 缺失仅 warn，服务以降级纯 API 模式继续
  - 文件：`src/app/mod.rs`
- 当生产环境需要收紧跨域时，
  - 解决：`config.cors_origins` 非空时只放行列表内 origin；为空则全放行 + warn
  - 文件：`src/app/mod.rs`、`src/config.rs`
- 当请求试图穿越目录时，
  - 解决：入参校验，单测覆盖 `..` 与绝对路径攻击串
  - 文件：`src/app/routes.rs`（校验逻辑在 `src/utils/mod.rs`）
- 当默认工具链过旧导致构建失败时，
  - 解决：`rust-toolchain.toml` 固定 `channel = "stable"`（进目录即自动切到最新 stable）
  - 文件：`rust-toolchain.toml`
- 当并发回测打满 worker 时，
  - 解决：重活进 `spawn_blocking` + `Semaphore` 闸门限并发
  - 文件：`src/app/routes.rs`
- 当进程被信号杀死截断在途请求时，
  - 解决：`with_graceful_shutdown` 等在途请求跑完
  - 文件：`src/app/mod.rs`
- 当刷新页面丢失周期/指标/策略/绘图时，
  - 解决：`last_session` + 按交易对绘图组持久化到 SQLite，启动恢复；命名布局可显式保存多套
  - 文件：`frontend/src/app.ts`、`src/app/state.rs`、`migrations/001_init.sql`

## 完整架构

```text
src/main.rs         # 入口：dotenv(CWD) → tracing → Config::load → run
src/lib.rs          # 只做导出 + crate 级 lint + README 内联文档
src/app/mod.rs      # run(config)：校验/状态/路由/CORS/静态/serve/优雅停机
src/app/routes.rs   # AppState(DataLoader + 状态库 + 回测缓存 + 并发闸门) + ApiResponse + 行情/状态路由 + 穿越防护
src/app/data_loader.rs # MarketType + KlineColumns(SoA) + DataLoader：kline-reader + moka
src/app/state.rs    # StateStore：layouts/drawings/preferences + 限额校验 + SQLite
src/app/strategy.rs # TOML-in → vta 回测：白名单 + 网格预检/语义校验 + 全网格逐档回测 → DTO（overlays 与信号同源 vector-ta 函数）
migrations/001_init.sql # layouts/drawings/preferences 表
src/config.rs       # Config{host/port/data_dir/db_path/frontend_dir/cors_origins} + load/load_from + validate
src/error.rs        # AppError(Io/Config/ConfigRead/ConfigParse/Data/State/InvalidAddr, non_exhaustive)
src/utils/mod.rs    # valid_pair/valid_timeframe/has_kline_files/has_data_files/timeframe_order
frontend/src/       # api/app/chart/indicators/main/types + style.css + vite-env
```

## 完整配置

```toml
# config/config.toml
host = "127.0.0.1"           # 可选，默认 127.0.0.1，监听地址
port = 3001                  # 可选，默认 3001
data_dir = "/data/nutilustrader/data"   # 必填，无默认，数据根
db_path = "/data/nutilustrader/data/tradingview.db"  # 可选，默认 <data_dir>/tradingview.db
frontend_dir = "/data/nutilustrader/creats/tradingview/frontend/dist"   # 可选，默认 <workspace>/frontend/dist
cors_origins = []           # 可选，默认 [] = 全放行（仅本地开发）
```

- 全量示例见 `config/config.example.toml`。

## 完整执行逻辑

- src/main.rs
  - 入口
  - 读 CWD/.env → `tracing_subscriber::fmt`（`TRADINGVIEW_LOG` > `RUST_LOG` > `tradingview_backend=info`）→ `Config::load()` → `run(&config)`
- src/app/mod.rs
  - `run(&Config)` 唯一编排：校验 `data_dir` → 打开 `StateStore` → 建 `AppState` → 路由+CORS+静态 → `bind+serve`（优雅停机）
- src/config.rs
  - `load()`：按优先级定位配置文件 → `toml::from_str` → 缺省补齐 → `validate()`
- src/error.rs
  - `AppError`（thiserror, non_exhaustive）：Io/Config/ConfigRead/ConfigParse/Data/State/InvalidAddr
- src/app/routes.rs
  - 行情/回测 GET：health / market-types / pairs / timeframes / klines / strategies / strategy
  - 状态：`/api/layouts`、`/api/layouts/{id}`、`/api/drawings`、`/api/preferences`
- src/app/state.rs
  - SQLite CRUD + 输入限额 + `spawn_blocking` 调用边界；布局/绘图/偏好三类状态
- src/app/data_loader.rs
  - `MarketType` 解析 → 数据文件定位（binance zip / dukascopy bi5）→ kline-reader 读 → 去非有限值/去重 → moka 存 → 返回列集
- src/app/strategy.rs
  - TOML-in → vta 回测：策略名注册 + 反序列化 + 白名单/网格预检/语义校验 + 全网格逐档回测 → DTO
- src/utils/mod.rs
  - 参数白名单校验 + 数据文件判别 + 周期排序
- frontend/
  - `api.ts` 调 `/api/*` → `chart.ts`（lightweight-charts 5 单 chart + 原生 pane + DrawingManager）渲染 → `indicators.ts`（technicalindicators）叠加 → `app.ts` 负责会话/布局/绘图自动保存

## 命令

```bash
cargo fmt
cargo check -p tradingview-backend --all-targets
cargo clippy -p tradingview-backend --all-targets -- -D warnings
cargo test -p tradingview-backend
cargo run -p tradingview-backend                  # 读 config/config.toml，:3001 对外
cd frontend && npm install && npm run dev         # 前端 dev（:5173，/api 代理到 :3001）
cd frontend && npm run build                       # 产物进 frontend/dist，被后端同服
```

> 工具链由 `rust-toolchain.toml` 固定为 `channel = "stable"`，上述命令无需手工指定版本号。

## 配置

见 `config/config.example.toml`（全字段）与 `config/config.toml`（当前生效）。
