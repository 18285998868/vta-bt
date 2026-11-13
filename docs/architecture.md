# 架构与执行流程

## 模块布局

```text
src/
  main.rs              # 入口: cli::parse → -c 回测 / --pbo 实算, 二选一; anyhow 收敛
  lib.rs               # 只做导出: pub mod app/cli/config/data/engine/error/money/output/pbo/strategies
  cli.rs               # clap 4 derive: --config/-c 与 --pbo 互斥; --no-cache 逃生门
  money.rs             # Amount(rust_decimal 定点十进制): 钱的唯一类型
  error.rs             # thiserror AppError(Io/Config/BinanceData/DukascopyData/Parquet/Arrow/Msg)
  data.rs              # DataSection::for_pair → reader 的 DataConfig; KlineColumns
  pbo.rs               # CSCV 实算 + 单测
  app/                 # 编排层
    run.rs             # run/run_with: 配置 → 作业 → 双层串行取数 → run_combo → merge
    combo.rs           # run_combo: 缓存判定 / 参数分批并行 / 写盘
    fingerprint.rs     # 数据指纹 + 源码内容哈希
    types.rs           # RunResult / RunStatus / DetailResult
  config/              # 原始 TOML → 校验 → 类型化配置
    load.rs            # load_config
    strategy.rs        # StrategyConfig 与参数网格
    data.rs / backtest.rs / output.rs
  engine/              # 回测引擎
    mod.rs             # simulate 系列公开入口
    exec.rs            # ExecConfig / Side
    stops.rs           # StopConfig / ATR / 保本止损
    sizing.rs          # SizingConfig / 开仓数量
    scan.rs            # 信号扫描、成交与平仓结算
    accum.rs           # 交易/权益曲线在线累计
    metrics.rs         # 指标计算
    types.rs           # SummaryMetrics / Trade / SimResult / ExitReason
  output/
    parquet/           # Parquet 读写
      report.rs        # 汇总报告构建与原子写入
      detail.rs        # 逐笔明细/权益曲线/侧车清单
      read.rs          # 缓存校验读取、合并
      params.rs        # 参数串规范化与编码
      columns.rs / schema.rs  # Arrow 列读取辅助 / 单位表
  strategies/
    mod.rs             # Strategy trait + shared 再导出
    shared.rs          # Signal / StopLines / price_cross_signal / bars_per_year
    build.rs           # build_one 穷尽 match → build_jobs_checked
    <name>/            # 每个策略: config.rs + strategy.rs
```

`tradingview/` 是独立 workspace，不参与根 crate 的构建。

## 逻辑编排

```text
启动 -> main.rs: cli::parse() 解析 --config/-c(或 --pbo <reports_dir>, 二选一)
v
加载配置 -> config::load_config: TOML 反序列化 → Config(deny_unknown_fields) + validate
v
构建作业 -> build_jobs_checked(config.strategy): 穷尽 match 覆盖全部变体 + 参数网格两层唯一性自检
v
for symbol in symbols                        # 品种层(串行)
v
  for timeframe in timeframes               # 周期层(串行)
v
    pair.load()                             # 按 source 调对应 reader → KlineColumns; 失败打印[跳过]并 continue
v
    for job in jobs                         # 策略层(串行)
v
      run_combo(job, symbol, tf, cols, ctx):
        a. 非 overwrite 且组合缓存 temps/{sym}_{tf}_{stg}_{参数指纹}.parquet 可读
           [+ detail: 侧车清单/文件大小/combo 元数据全部校验通过] → 跳过(仍并入合并)
        b. 按信号内存预算分批, 批内 (0..n).into_par_iter: 生成信号 → simulate(/detailed)
        c. detail 模式收集 trades/equity_curve → detail_rows
        d. write_single_parquet(temps/) [+ write_detail_parquet(detail/) + 侧车清单]
v
合并 -> merge_parquet_files(combo_files) → {out_dir}/result.parquet   # 只合并本次组合(新算+跳过), 整文件重建
detail 模式 -> write_detail_index(detail/manifest.toml)                # 只列本轮 fixed 曲线
```

- 单个 `(symbol, timeframe)` 数据加载失败只跳过该对；全部组合失败才返回错误。
- 三个外层循环**刻意串行**（内存约束，见 `./invariants.md` 第 1 条），只有参数索引一层并行。
- `data_fingerprint(cols)` 在策略层循环外算一次，供该帧全部策略复用。

## 实现要点

### main.rs / cli.rs

- `main`：tracing 初始化（`RUST_LOG`，缺省 info）→ `cli::parse()` → `(config, pbo)` 二选一：`-c` 走回测，`--pbo <reports_dir>` 走 CSCV 实算，否则 `bail`。
- `cli`：`--config/-c` 与 `--pbo` 互斥必填其一；`--no-cache` 忽略组合缓存。

### 加载配置与校验

解析层：文件可读（失败带路径）→ TOML 可解析（失败带路径）→ `deny_unknown_fields` 拒收未知键（`PeriodParam` untagged 除外）。

语义层：`initial_balance > 0`；`symbol` / `timeframe` / `strategy` 非空；周期与止损参数范围合法。

### 数据加载（dir 为根）

`DataSection::for_pair(symbol, tf)` 降为 `data::DataConfig`（`dir` 原样透传，reader 自动追加数据源子路径）→ `load()`；失败打印 `[跳过]` 并 `continue`，该帧全部策略复用同一份 `KlineColumns`，帧末释放。

### run_combo（缓存 + 参数并行 + 写盘）

`params_fingerprint()`（FNV-1a 混入 balance/window/delay/fee/slip/fill + detail/fixed_curves 开关 + 各 `param_str`）定位 `temps/{sym}_{tf}_{stg}_{fp:016x}.parquet`：

- `overwrite = false` 且可读即跳过；`detail` 模式还要求侧车 `{stem}.detail.toml` 及全部产物存在、大小/combo 元数据一致，否则重算该组合。
- 未命中则按 `DETAIL_MEM_BUDGET` 估算分批，批内 `(0..n).into_par_iter` 生成信号 + 模拟一波完成。
- 产出集开关进指纹的理由见 `./invariants.md` 第 7 条。

### 合并

`merge_parquet_files(combo_files)` 只合并本次组合（新算 + 跳过），整文件重建 `result.parquet`；失败文件数进最终摘要，全部失败才报错。

## 完整执行逻辑

- `main.rs`：入口；tracing 初始化 → `cli::parse()` → `-c` 回测 / `--pbo` 实算。
- `cli.rs`：clap 4 derive，`--config/-c` 与 `--pbo` 二选一。
- `config/load.rs`：`load_config` 读文件 → `toml` 解析 → `validate`。
- `config/data.rs`：`for_pair` 多值降单值，供 `DataConfig::load` 使用。
- `error.rs`：`AppError`（thiserror）统一错误类型。
- `app/run.rs`：`run_with` 预算目录 → 双层串行 → `pair.load()`（失败跳过）→ `run_combo` → `merge` → `result.parquet` → `detail/manifest.toml`。
- `app/combo.rs`：单组合缓存判定、分批并行、行构造（`run_row` 唯一构造点）与写盘。
- `engine/mod.rs`：`simulate` / `simulate_detailed` 与带 high/low 的 `simulate_ohlc*`：信号 → 持仓 → 成交 → 绩效。
- `output/parquet/`：`temps/` 单组合写 + `detail/` 明细写 + `read` 校验 + `merge`。
- `strategies/build.rs`：按 `[[strategy]]` 展开参数实例，并把该条的 `stop` 一并带入作业。
- `pbo.rs`：读 `detail/manifest.toml` 索引 → 筛 `status == ok` / 同时间网格 → 分组 CSCV → 报告。
