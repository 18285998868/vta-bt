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
- 指标计算：`vector-ta`（**workspace path 依赖**，不是 crates.io —— 换 patch 版本会改变 NaN 前缀长度，进而改变全部历史回测结果）
- 数据读取：自研 `kline-reader`（workspace path 依赖）
- MSRV：`rust-version = "1.97"`（`pbo.rs` 的 Gosper hack 用到 1.97 稳定的 `isolate_lowest_one`）
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
fixed_curves = false # 可选，是否写 *_fixedcurve.parquet（--pbo 的 T×N 输入），默认 false
overwrite = true # 可选，是否覆盖已存在结果，默认 false（跳过已完成组合）

[[strategy]]
type = "sma" # 必填
period = { start = 5, end = 100, step = 5 } # 必填，参数范围；单值可写 period = 20
```

## 关键设计约束（改动前必读，13 条）

这些不是风格偏好，而是踩过坑之后定下来的硬约束。违反它们会**静默产出错误结果**。

### 1. 外层三层循环必须串行（品种 × 周期 × 策略）

`app/mod.rs::run` 的三个外层循环**刻意保持串行**，不得"优化"为并行：

- `cols`（一个 (symbol, tf) 的全部 K 线）在 1m 级数据下约 125MB（2.6M bar × 6 数组 × 8B）。并行加载会直接 OOM。
- 每个策略的 `generate_signals_batch` 会常驻整个 chunk 的 `Vec<Vec<Signal>>`，单策略即可打满 4GiB 预算。策略层并行会让内存乘以策略数。

只有**参数索引**一层并行（rayon `into_par_iter`）。这是内存约束下的唯一安全形态。

### 2. `delay_bars` 恒 ≥ 1（类型级保证）

`ExecConfig::delay_bars` 是 `NonZeroUsize`，`delay_bars = 0`（信号当根成交）属前视乐观口径，已在**引擎层彻底删除**，`try_from_bps_fill(0, ..)` 直接返回 `Err`。配置层 `Config::validate` 亦拒绝。

### 3. `generate_signals_full` 的形参顺序由测试锁定

形参是 `&self, open, high, low, close, volume, time, param_idx`，**全部 `&[f64]`，编译器无法发现错位**。
历史故障：移除 close-only 入口时把 `high`/`low` 插到 `open` 之前 → high/low 的数据被喂进 `open`，静默产出错误信号且无任何报错。

`strategies::arg_order_tests::test_full_arg_order_contract` 用等长但内容差异极大的切片做探针锁定该契约。**新增或改签名时必须让该测试通过。**

### 4. 权益曲线穿零（空头浮空）按 0 收益处理

空头持仓期间权益可能 ≤ 0，此时 `ln(curr/prev)` 返回 NaN；NaN 经 Welford 污染均值/方差后会让 `ret_m2 > 0.0` 守卫失效，导致 **sharpe/volatility 被静默归零**、sortino 直接输出 NaN。

`engine::bar_log_returns` 是全部收益口径的唯一来源（含 `pbo.rs`），两侧都要检查 `prev <= 0` 与 `curr <= 0`。**不要绕过它直接调 `.ln()`。**

### 5. 指标口径与**单位**（报告引用前请确认）

| 指标 | 口径 | 备注 |
|---|---|---|
| `sharpe` | 逐 bar **对数收益**，样本方差 ddof=1 × √bpy | |
| `sortino` | 下行偏差按**下行 bar 数** ddof=1 归一 | 与 sharpe 同一自由度约定，可并列比较 |
| `sqn` | `√n · mean/stdev`，**n 不封顶** | Van Thorp 1998。`√n` 是定义中有意的统计显著性因子；小样本由 `num_trades >= 30` 门槛约束（Van Thorp 建议），不改写公式 |
| `fixed_*` | 固定名义、不复利 | 研究报告统一引用 `fixed_*` 前缀列 |

**单位契约**（`parquet::COLUMNS`，schema 的单一来源）：

| 单位 | 含义 | 本报告中的列 |
|---|---|---|
| `Ratio` | **小数**比率 | `total_return`、`max_drawdown`、`win_rate`、`fixed_total_return`、`fixed_max_drawdown`、`buy_hold_return` |
| `Percent` | 百分数（×100） | `volatility_pct` |
| `Money` | 以初始资金计价的货币 | `balance`、`final_equity`、`avg_win`、`avg_loss`、`expectancy`、`fixed_pnl`、`buy_hold_pnl` |
| `Price` | 价格差（报价货币，**非金额**） | `avg_mae_price`、`avg_mfe_price` |
| `Bars` / `Count` | bar 数 / 整数计数 | `avg_hold_bars` / `trades`、`bars_used`、`delay_bars`、`consecutive_*` |
| `PerYear` | 次 / 年 | `turnover_per_year` |
| `BasisPoints` | 基点 | `fee_bps`、`slippage_bps` |
| `Dimensionless` | 无量纲非比率 | `sharpe`、`sortino`、`profit_factor`、`calmar`、`skewness`、`kurtosis`、`p_value` … |

**已修正的单位错配**：`max_drawdown` 一直是小数，而 `fixed_max_drawdown_pct`
原本也是百分数 —— 两条腿并排对比时 fixed 腿会凭空大 100 倍。现已统一为小数并改名
`fixed_max_drawdown`（同时删掉了名字里的 `_pct`）。

新增列若不在 `COLUMNS` 表里声明单位，`test_every_report_column_declares_its_unit`
会失败。

### 5b. 退化结果必须可被识别：`bars_used` + `status`

指标全 0 有两种截然不同的成因：**真的零波动低风险**，或**策略根本没跑起来**
（数据长度 < 策略窗口 → 全程 Hold）。实测反例：同一 `params` 串 `period=400`

- 1d / 300bar → `trades=0 / pnl=0 / sharpe=0`
- 1m / 20000bar → `trades=1310 / pnl=-1.0 / sharpe=-18.04`

`params` 只描述参数、不描述数据规模，两者无法区分。报告因此带两列：

- `bars_used`：本次求值实际喂给策略的 bar 数
- `status`：`ok` / `no_signals`（全程 Hold）/ `no_trades`（有信号但一笔未成交）

**读表时必须按 `status == "ok"` 过滤，并结合 `bars_used` 判断样本量。**
`no_signals` 同时覆盖"窗口 > 数据长度"与"该参数本就无信号"两种成因（引擎层
不可区分），`bars_used` 提供区分所需信息。

### 6. 参数网格的唯一性在构建期校验

`strategy_params` 整数列由 `strategy_params_value` 手工把参数元组压进 u32，溢出时回退 `fnv1a_32`（**有损**）。
行主键实为 `(symbol, timeframe, strategy, params)` 四元组，该列仅作"同策略内按参数大小排序"的代理，碰撞不构成数据损坏，但会让排序错乱。

`build_strategies_checked` 在每次启动时对展开后的完整网格做 `HashSet` 唯一性自检，碰撞即**报错退出**（O(N)，微秒级）。新增策略后若看到该错误，说明 `param_str` 格式写错了。

### 7. 缓存指纹覆盖数据身份

`temps/{...}_{指纹}.parquet` 的指纹 = FNV-1a(**行情内容哈希** + 资金 + 日期窗口 + 执行口径 + 逐参数 `param_str`)。

`data_fingerprint` 对六列的**原始位模式**做内容哈希（列名也混入，故增删列必然改指纹）。
用内容哈希而非目录 mtime/递归遍历：目录 mtime 只反映条目增删，覆盖不到"同路径文件被
原地改写"；列已全量载入内存，成本约 20ms/1M bars。

**为什么必须有它**：同一 `start~end` 窗口下先跑一次（当时该目录只有 1 月数据），
再补进 2 月数据重跑 —— 旧实现的 `window` 字符串一字未变，于是全部组合命中旧缓存，
报告里的 bars 数/指标仍基于只有 1 月的数据，而使用者以为跑的是完整窗口。
这类错误不会报错，只会让研究结论悄悄过期。

### 8. `strategy_params_json` 必须始终是合法 JSON

值统一为 JSON **字符串**并转义（`{"ma":"hma","malen":"100"}`）。历史实现对 `ma=`/`variant=` 形态输出裸 token（`{"ma":hma}`），
**非法 JSON 且与同列内合法的 `{"sigma":2.5}` 并存**，使整列无法被 JSON 工具消费。测试用 `serde_json` 实解析断言。

### 9. `read_parquet` 按**列名**索引，禁用魔法列号

`ColumnIndex` 把列名映射成下标：必需列缺失即**报错并列出实际列名**，可选列
（旧缓存没有的追加列）按名字查、查不到才回退。

历史实现用常量列号（`f64_col(&batch, 11)`），且用 `num_columns() > 37` 判断列存在
—— 每追加一列，后续列号全部偏移，而相邻同为 `Float64` 的列 `downcast_ref`
**不会 panic，而是静默读错列**（例如把 `sharpe` 读成 `sortino`）。

新增输出列仍应**追加到末尾**以兼容旧缓存，但正确性不再依赖这一点。

### 10. 缓存行数也是有效性的一部分

指纹相同**不保证**文件完整（可能被截断、被旧版本写入）。`run_combo` 要求缓存
行数等于 `strategy.param_count()`，不符即判失效重算。

### 11. `RunResult` 只有一个构造点

detail 与非 detail 两条分支共用 `ComboCtx::run_row`。此前两份 16 字段字面量
各自维护，任何新增字段都可能只改到其中一条。

### 12. 引擎不做无谓分配

- `CurveAccum`：`simulate()` 只算统计量，**不物化权益曲线**（O(1) 内存而非
  O(T)）。与"物化后单遍遍历"**逐位一致**，由
  `test_curve_accum_is_bit_identical_to_materialized_reference` 锁定。
- `CloseOutcome`/`TradeAccum`：非 detailed 路径不构造 `Vec<Trade>`，
  `exit_reason` 是 `&'static str`（原为每笔一个堆 `String`）。
  两条路径的指标必须逐位一致，由 `non_detailed_metrics_match_detailed_bit_for_bit`
  锁定。
- `Side` enum 取代 `i8` 方向：原实现里 `side == 2` 会静默走"卖出"分支。

### 13. K 线六列必须等长

引擎内部用 `close.len().min(open.len())` 截断，列短了只会少算几根 bar 而
**不报错**；`time` 短了更会让 `ts(i)` 退化成用 bar 序号当时间戳。
`assert_columns_aligned` 在进入回测前一次性拒绝。

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
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals_full/generate_signals_batch, Send+Sync) + shared 再导出
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
  - 解决：`DETAIL_MEM_BUDGET = 4GiB` 分批写盘；且 `simulate()` 路径不物化权益曲线
  - 文件：`src/app/mod.rs`
- 当某个参数全 Hold 时，报告里指标全 0，和"零波动低风险"无法区分，
  - 解决：加 `bars_used` + `status` 两列，读表按 `status == "ok"` 过滤
  - 文件：`src/app/mod.rs`
- 当某组组合文件读失败时，报告缺了参数却仍显示"完成"，
  - 解决：`merge_parquet_files` 返回 `MergeStats{failed_files}`，失败数进最终摘要；
    全部失败则报错
  - 文件：`src/app/04_write_output/parquet.rs`

## 完整架构

```
main.rs             # 入口：cli::parse → -c 回测 / --pbo 实算，二选一；anyhow 收敛
lib.rs              # 只做导出：#[path] 挂载 app/NN_*（config/data/engine/output/strategies）+ pub mod app/cli/error
cli.rs              # clap 4 derive，--config/-c 与 --pbo 互斥
config.rs           # Config{data/backtest/output/strategy} + DataSection::for_pair 降单值
error.rs            # thiserror AppError(Io/Config/Data/Parquet/Arrow/Msg)
pbo.rs              # CSCV 实算 + 单测
app/mod.rs          # run/run_combo：validate → build → 双层串行取数 → 策略层 → temps/detail → merge result.parquet
app/03_run_backtest.rs # engine：simulate/simulate_detailed + CurveAccum(在线统计) + SummaryMetrics
app/04_write_output/parquet.rs # temps/detail 写 + ColumnIndex(按列名读) + 单位契约 COLUMNS + merge
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
