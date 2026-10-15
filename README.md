# vta

## 项目目标

> 对多种技术分析指标 × 参数组合做向量化批量回测，一次遍历 `品种 × 时间帧 × (指标 × 参数范围)` 的笛卡尔积，输出 Parquet 报告（`result.parquet` 合并结果 + 可选 `detail/` 逐笔交易/权益曲线）。
>
> - 数据：自研 `kline-reader`（workspace path 依赖，统一读取 binance/dukascopy K 线）；`dir` 为**数据根目录**（程序按数据源自动追加子路径）。
> - 并行：rayon（单策略的参数索引按 `(0..n).into_par_iter` 并行，信号生成与回测同为一遍）。
> - 去重执行：`overwrite = false` 且 `temps/{sym}_{tf}_{stg}_{参数指纹}.parquet` 可读即跳过（仍并入合并）。
> - 另有 `--pbo` 模式：按（币，周期，策略）分组实算 CSCV（Bailey et al. 2016）。

## 技术选型

- rust
- edition = "2024"
- 指标计算：`vector-ta` 0.3（crates.io）
- 数据读取：自研 `kline-reader`（workspace path 依赖）
- 并行：rayon（参数索引 `into_par_iter`）
- 输出：arrow 60 + parquet 60
- 配置：clap 4（derive）+ toml 1.1 + serde（derive）
- 错误：thiserror 2（`AppError`）；入口 anyhow 收敛
- 发布配置：`lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 冒烟配置

```toml
[data]
type = "binance" # 必填，数据源类型："binance" | "dukascopy"
dir = "/data/nutilustrader/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = ["BTCUSDT"] # 必填，回测品种列表
timeframes = ["1d"] # 必填，可选 ["1m","3m","5m","15m","30m","1h","2h","4h","6h","8h","12h","1d"]
# start_date = "2024-01-01" # 可选，数据起始日期，格式 YYYY-MM-DD
# end_date = "2024-12-31" # 可选，数据结束日期，格式 YYYY-MM-DD

[backtest]
initial_balance = 10000.0 # 必填，初始资金（>0）

[output]
dir = "/tmp/vta-smoke" # 可选，输出目录，默认 "reports"
detail = true # 可选，是否导出逐笔交易/权益曲线，默认 false
overwrite = true # 可选，是否覆盖已存在结果，默认 false（跳过已完成组合）

[[strategy]]
type = "sma" # 必填
period = { start = 5, end = 100, step = 5 } # 必填，参数范围；单值可写 period = 20
```

## 逻辑编排

```
启动 -> main.rs: cli::parse() 解析 --config/-c（或 --pbo <reports_dir>，二选一）
v
加载配置 -> config::load_config：toml 反序列化 → Config(deny_unknown_fields) + validate(资金>0 / symbol / timeframes / strategy 非空)
v
构建策略 -> build_strategies(config.strategy)：MA 变体(build_ma! 宏) + ssf/ssf_st → Vec<Box<dyn Strategy>>
v
for symbol in symbols                        # 品种层（串行）
v
  for timeframe in timeframes               # 时间帧层（串行）
v
    load_columns(symbol, timeframe)          # kline-reader：dir 为根 → KlineColumns；失败打印[跳过]并 continue
v
    for strategy in strategies               # 策略层（串行）
v
      run_combo(strategy, symbol, tf, cols, ctx):
        a. 非 overwrite 且缓存 temps/{sym}_{tf}_{strat}_{参数指纹}.parquet 可读 → 跳过（仍并入合并）
        b. (0..n).into_par_iter 参数并行：一波 生成信号 → simulate(/detailed)
        c. detail 模式收集 trades/equity_curve → detail_rows
        d. 写出 write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
v
合并 -> merge_parquet_files(combo_files) → {out_dir}/result.parquet   # 只合并本次运行组合（新算+跳过），整文件重建
```

## 架构设计

```
# 支撑件(cli/error/pbo)在 crate 根，不编号；执行代码在 app/，按"调用执行流程"编号：main → cli → 01_config → 02_load_data → 03_run_backtest → 04_write_output → 05_strategies
main.rs             # 入口：cli::parse() → -c 走 config::load_config → app::run(&config)，--pbo 走 pbo::run_pbo_report；二选一，错误 anyhow 收敛
lib.rs              # 只做导出：#[path] 挂载 app/NN_* 保持逻辑模块名(config/data/engine/output/strategies) + pub mod app/cli/error
cli.rs              # 支撑件：命令行解析 clap 4(derive)，解析 --config/-c（必填，无默认值）+ --pbo，返回 Cli
config.rs           # 支撑件：Config{data/backtest/output/strategy} + DataSection::for_pair(symbol, tf) 降为 kline-reader 单值 DataConfig
error.rs            # 支撑件：AppError(thiserror, Io/Config/Data/Parquet/Arrow/Msg)，#[from] 实现 ? 自动转换
pbo.rs              # 支撑件：CSCV 实算（Bailey et al. 2016），T×N 常驻内存
app/                            # 执行代码
app/mod.rs          # 整体编排 run(config)：validate → build_strategies → 预算 out_dir → for symbol × timeframe(串行) → load_columns → for strategy: run_combo → merge → result.parquet
app/03_run_backtest.rs # 回测引擎（lib 映射为 engine）：持仓/成交/绩效 — SummaryMetrics(30 项)/Trade/SimResult；simulate / simulate_detailed 供 run_combo 单参数调用
app/04_write_output/    # 输出（lib 映射为 output）
app/04_write_output/parquet.rs # Parquet 读写：write_single_parquet(单组合 temps/) / write_detail_parquet(detail/) / read_parquet(缓存校验) / merge_parquet_files(显式合并)
app/05_strategies/      # 策略叶（lib 映射为 strategies，~80+ 子目录）
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals, Send+Sync) + shared 再导出
app/05_strategies/shared.rs # Signal 枚举(Hold/Buy/Sell/Close) / price_cross_signal 穿越信号 / bars_per_year
app/05_strategies/build.rs # 策略构建工厂(build_ma! 宏 MA 变体 → Box<dyn Strategy>) + ssf/ssf_st
app/05_strategies/{alma,dema,...,zlema}/… # 单 MA 策略（周期遍历 → vector_ta 指标 → 价格穿越）
```

## 实现

### main.rs / cli.rs

- `main`：tracing 初始化（`RUST_LOG`，缺省 info）→ `cli::parse()` → `(config, pbo)` 二选一：`-c` 走回测，`--pbo <reports_dir>` 走 CSCV 实算，否则 `bail`。
- `cli`：`--config/-c` 与 `--pbo` 互斥必填其一。

### app

#### 加载配置&校验

解析层：

1. 文件可读，失败带路径。
2. TOML 可解析，失败带路径。
3. `deny_unknown_fields`：未知键拒收（`PeriodParam` untagged 除外）。

语义层：`initial_balance > 0`；`symbol` / `timeframes` / `strategy` 非空。

#### 数据加载（dir 为根）

`DataSection::for_pair(symbol, tf)` 降为 kline-reader 单值 `DataConfig`（`dir` 原样透传，kline-reader 自动追加数据源子路径）→ `load_columns`；失败打印 `[跳过]` 并 `continue`，供该帧全部策略复用，帧末释放。

#### run_combo（缓存 + 参数并行 + 写盘）

`params_fingerprint()`（FNV-1a 混入 balance/window/delay/fee/slip/fill + 各 param_str）定位 `temps/{sym}_{tf}_{stg}_{fp:016x}.parquet`：`overwrite = false` 且可读即跳过；否则 `(0..n).into_par_iter` 生成信号+模拟一波完成；`detail` 模式按 `DETAIL_MEM_BUDGET = 4GiB` 分批（`per_param = len×200B`）防爆内存。

#### 合并

`merge_parquet_files(combo_files)` 只合并本次组合（新算 + 跳过），整文件重建 `result.parquet`。

## PBO 模式

```bash
# 1. 先用 detail=true 跑出逐笔 fixed 曲线（detail/ 下 *_fixedcurve.parquet）
# 2. 按（币，周期，策略）分组实算 CSCV（Bailey et al. 2016，S=16→12870 logits）
cargo run --release -- --pbo /path/to/reports_XXX
```

- 与 `-c` 二选一；T×N 常驻内存，1m×500 级约 24GB——PBO 建议 TF≥30m 或小 N。
- T<32 或 N<10 的组跳过并明示；φ>0.05 按 Neyman-Pearson 惯例拒绝。
- 源码：`src/pbo.rs`（纯 CSCV + 单测：噪声φ高/植入边际φ低）+ detail fixedcurve 写读。

## 问题

- 当 `dir` 写成内层 klines 路径时，报目录不存在，
  - 解决：`dir` 统一为**数据根**（如 `/data/nutilustrader/data`），kline-reader 自动追加子路径；不要手写 `binance/...` 进去
- 当某组数据加载失败时，整批中断，
  - 解决：打印 `[跳过]` 并 `continue`，仅该 symbol×timeframe 被跳过；全部失败则报错提示检查 `data.dir/symbol/timeframe`
  - 文件：`src/app/mod.rs`
- 当重复跑大组合时，全量重算太慢，
  - 解决：参数指纹缓存 + `overwrite = false` 跳过已完成组合（仍并入合并）
  - 文件：`src/app/mod.rs`（`params_fingerprint`/`run_combo`）
- 当 detail 大参数量爆内存时，
  - 解决：`DETAIL_MEM_BUDGET = 4GiB` 分批写盘
  - 文件：`src/app/mod.rs`

## 完整架构

```
main.rs             # 入口：cli::parse → -c 回测 / --pbo 实算，二选一；anyhow 收敛
lib.rs              # 只做导出：#[path] 挂载 app/NN_*（config/data/engine/output/strategies）+ pub mod app/cli/error
cli.rs              # clap 4 derive，--config/-c 与 --pbo 互斥
config.rs           # Config{data/backtest/output/strategy} + DataSection::for_pair 降单值
error.rs            # thiserror AppError(Io/Config/Data/Parquet/Arrow/Msg)
pbo.rs              # CSCV 实算 + 单测
app/mod.rs          # run/run_combo：validate → build → 双层串行取数 → 策略层 → temps/detail → merge result.parquet
app/03_run_backtest.rs # engine：simulate/simulate_detailed + SummaryMetrics(30 项)
app/04_write_output/parquet.rs # temps/detail 写 + read 校验 + merge
app/05_strategies/  # Strategy trait + shared + build + ~80 策略叶
```

## 完整配置

```toml
[data]
type = "binance" # 必填，"binance" | "dukascopy"
dir = "/data/nutilustrader/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = "BTCUSDT" # 必填，单值或数组
timeframe = "1d" # 必填，单值或数组
# start_date = "2024-01-01" # 可选
# end_date = "2024-12-31" # 可选
[backtest]
initial_balance = 10000.0 # 必填，>0
# delay_bars / fill_bar_price(open|close) / fee_bps / slippage_bps 均可选，默认 0/open/0/0
[output]
dir = "/data/nutilustrader/outputs/reports/vta" # 可选，默认 "reports"
detail = false # 可选，默认 false
overwrite = false # 可选，默认 false
[[strategy]] # 必填，至少一个
type = "sma"
period = { start = 5, end = 100, step = 5 } # 范围或单值
```

- 未知键拒收；读写/解析失败带文件路径。
- 全量示例见 `config.example.toml`；`config.ma_all_1h.toml` / `config.sma_100.toml` 为常用组合；`scripts/` 下有定稿配置。

## 完整执行逻辑

- main.rs
  - 入口
  - tracing 初始化 → `cli::parse()` → `-c` 回测 / `--pbo` 实算
- cli.rs
  - clap 4 derive 解析，`--config/-c` 与 `--pbo` 二选一
- config.rs
  - `load_config`：读文件 → `toml` 解析 → `validate`（资金>0/symbol/timeframes/strategy 非空）
  - `for_pair`：多值降单值，供 `load_columns` 使用
- error.rs
  - `AppError`（thiserror）：Io/Config/Data/Parquet/Arrow/Msg
- app/mod.rs
  - `run`：预算目录 → 双层串行 → `load_columns`（失败跳过）→ `run_combo`（缓存/并行/写盘）→ `merge` → `result.parquet`
- app/03_run_backtest.rs
  - `simulate`/`simulate_detailed`：信号→持仓→成交→30 项绩效
- app/04_write_output/parquet.rs
  - `temps/` 单组合写 + `detail/` 明细写 + `read` 校验 + `merge`
- app/05_strategies/
  - `build.rs` 工厂按 `[[strategy]]` 展开参数实例
- pbo.rs
  - 读 `detail/` fixed 曲线 → 分组 CSCV → 报告

## 命令

```bash
cargo check -p vta
cargo clippy -p vta --all-targets
cargo test -p vta
cargo fmt
cargo run -p vta --release -- -c config.sma_100.toml  # 冒烟
cargo run -p vta --release -- --pbo /path/to/reports  # PBO 实算
```

## 配置

见 `config.example.toml`（全字段）与 `config.sma_100.toml`（冒烟最小集）。
