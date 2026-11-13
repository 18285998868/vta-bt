# vta

## 项目目标

> 对多种技术分析指标 × 参数组合做向量化批量回测，一次遍历 `品种 × 时间帧 × (指标 × 参数范围)` 的笛卡尔积，输出 Parquet 报告（`result.parquet` 合并结果 + 可选 `detail/` 逐笔交易/权益曲线）。
>
> - 数据：自研 `binance-data-reader` / `dukascopy-data-reader`（workspace path 依赖，分别读取两源 K 线）；`dir` 为**数据根目录**（程序按数据源自动追加子路径）。
> - 并行：rayon（单策略的参数索引按 `(0..n).into_par_iter` 并行，信号生成与回测同为一遍）。
> - 去重执行：`overwrite = false` 且 `temps/{sym}_{tf}_{stg}_{参数指纹}.parquet` 可读即跳过（仍并入合并）。
> - 另有 `--pbo` 模式：按（币，周期，策略）分组实算 CSCV（Bailey et al. 2016）。

## 快速开始

```bash
cargo run -p vta --release -- -c config.sma_100.toml   # 冒烟
cargo run -p vta --release -- --pbo reports            # PBO 实算
```

最小配置（字段含义见 `docs/configuration.md`）：

```toml
[data]
type = "binance" # 必填，数据源类型："binance" | "dukascopy"
market = "futures/um" # 可选，Binance 市场："spot" | "futures/um" | "futures/cm"，默认 futures/um
dir = "path/to/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = ["BTCUSDT"] # 必填，回测品种列表
timeframe = ["1d"] # 必填(单数!)，可选 ["1m","3m","5m","15m","30m","1h","2h","4h","6h","8h","12h","1d","1w"]
# start_date = "2024-01-01" # 可选，数据起始日期，格式 YYYY-MM-DD
# end_date = "2024-12-31" # 可选，数据结束日期，格式 YYYY-MM-DD

[backtest]
initial_balance = 10000.0 # 必填，初始资金（>0）

[output]
dir = "/tmp/vta-smoke" # 可选，输出目录，默认 "reports"
detail = true # 可选，是否导出逐笔交易/权益曲线，默认 false
fixed_curves = false # 可选，是否写 *_fixedcurve.parquet（--pbo 的 T×N 输入）；依赖 detail = true，默认 false
overwrite = true # 可选，是否覆盖已存在结果，默认 false（跳过已完成组合）

[[strategy]]
type = "sma" # 必填
period = { start = 5, end = 100, step = 5 } # 必填，参数范围；单值可写 period = 20
```

## 仓库结构

| 路径 | 内容 |
|---|---|
| `src/` | 回测主 crate，模块布局见 `docs/architecture.md` |
| `tests/` | 集成测试与文档一致性测试 |
| `docs/` | 架构、配置、不变量、PBO 与审查记录 |
| `tradingview/` | TradingView 风格本地研究终端（独立 workspace），见 `tradingview/README.md` |

`tradingview/` 自带 `Cargo.toml` / `Cargo.lock`，与根 crate 互不参与对方的 workspace；在各自目录内执行 `cargo` 命令。

## 技术选型

- rust，edition 2024
- 指标计算：`vector-ta`（crates.io **0.3.1**，0.3.2 尚未发布；版本由 `Cargo.lock` 锁定 —— 换 patch 版本会改变 NaN 前缀长度，进而改变全部历史回测结果）
- 数据读取：自研 `binance-data-reader` / `dukascopy-data-reader`（workspace path 依赖）
- Rust 版本：**始终使用最新 stable**（`rust-toolchain.toml` 的 `channel = "stable"`，无需人工跟进版本号；刻意不声明 `rust-version` 下限，以免 `clippy::incompatible_msrv` 拦下新 API）
- 并行：rayon（参数索引 `into_par_iter`）
- 输出：arrow 60 + parquet 60
- 配置：clap 4（derive）+ toml 1.1 + serde（derive）
- 错误：thiserror 2（`AppError`）；入口 anyhow 收敛
- 发布配置：`lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 文档

| 文档 | 内容 |
|---|---|
| `docs/architecture.md` | 模块布局、逻辑编排、实现要点 |
| `docs/configuration.md` | 完整配置、持仓期止损、策略出场 |
| `docs/invariants.md` | 关键设计约束（改动前必读） |
| `docs/pbo.md` | CSCV / PBO 模式与分块口径 |
| `docs/reviews.md` | 2026-10 代码审查修复记录与设计决策复核 |

## 命令

```bash
cargo check -p vta
cargo clippy -p vta --all-targets
cargo test -p vta
cargo fmt
cargo run -p vta --release -- -c config.sma_100.toml  # 冒烟
cargo run -p vta --release -- --pbo /path/to/reports  # PBO 实算
```

完整门禁见 `scripts/check.sh`，本地与 CI 共用同一入口。

## 配置

见 `config.example.toml`（全字段）与 `config.sma_100.toml`（冒烟最小集）；字段与止损语义见 `docs/configuration.md`。

另有两个**审查用冒烟配置**（规模小、覆盖多条关键路径，供回归核对）：

| 文件 | 覆盖点 |
|---|---|
| `config.review_smoke.toml` | 多品种 × 多周期 × 多策略 + `detail` + `fixed_curves` + `--pbo` 全链路 |
| `config.review_wide.toml` | 本轮审查触及的 16 个策略（`pct_b` 家族 / BB 周期常量化 / `vwap` 长度口径 / 网格封顶 / `ssf` 校验下沉 …） |
| `config.review_invalid.toml` | 验证「参数不可计算」在报告里显式可见（`status = "invalid"`） |
| `config.review_bblen.toml` | 验证 `bblen` 成为独立网格维度（σ × BB 周期） |

这些配置写死了 `data.dir = "/data/data"`，换机器需改；输出目录使用相对路径 `reports/`（已 gitignore），随仓库走。

## 常见问题

- 当 `dir` 写成内层 klines 路径时，报目录不存在。
  - 解决：`dir` 统一为**数据根**（如 `path/to/data`），reader 自动追加子路径；不要手写 `binance/...` 进去。
- 当读取 spot / CM 数据却未设置 `market` 时，会按默认 `futures/um` 查找目录。
  - 解决：显式设置 `market = "spot"` 或 `market = "futures/cm"`；`futures/um` 可省略。
- 当某组数据加载失败时，整批中断。
  - 解决：打印 `[跳过]` 并 `continue`，仅该 symbol×timeframe 被跳过；全部失败则报错提示检查 `data.dir/symbol/timeframe`。
- 当重复跑大组合时，全量重算太慢。
  - 解决：参数指纹缓存 + `overwrite = false` 跳过已完成组合（仍并入合并）；必要时用 `--no-cache` 强制重算。
- 当 detail 大参数量爆内存时。
  - 解决：`DETAIL_MEM_BUDGET` 分批写盘；且 `simulate()` 路径不物化权益曲线。
- 当某个参数全 Hold 时，报告里指标全 0，和"零波动低风险"无法区分。
  - 解决：加 `bars_used` + `status` 两列，读表按 `status == "ok"` 过滤。
- 当某组组合文件读失败时，报告缺了参数却仍显示"完成"。
  - 解决：`merge_parquet_files` 返回 `MergeStats{failed_files}`，失败数进最终摘要；全部失败则报错。
