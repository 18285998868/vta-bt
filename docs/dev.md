# vta (vector-ta) 项目开发

## 需求分析

- 项目目标: 对多种技术分析指标 × 参数组合做向量化批量回测,一次遍历 `品种 × 时间帧 × (指标 × 参数范围)` 的笛卡尔积,输出 Parquet 报告(`result.parquet` 合并结果 + 可选 `detail/` 逐笔交易/权益曲线)。

## 技术选型

- 语言: rust, edition = "2024"
- 指标计算: `vector-ta` 0.3(crates.io)
- 数据读取: 自研 `lib-reader`(workspace path 依赖,统一读取 binance/dukascopy K 线)
- 并行: rayon(单策略的参数索引按 `(0..n).into_par_iter` 并行,信号生成与回测同为一遍)
- 输出: arrow 59 + parquet 59
- 配置: clap 4(derive) + toml 1.1 + serde(derive)
- 错误: thiserror 2(`AppError`);入口仍用 anyhow 收敛
- 发布配置: `lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 冒烟配置

```toml
# ========================================
# 数据源配置
# ========================================
[data]
# 必须, 数据源类型: "binance" | "dukascopy"
type = "binance"
# 必须, K线数据目录
dir = "/data/nutilustrader/data/futures/um/daily/klines"
# 必须, 回测品种列表
symbol = ["BTCUSDT"]
# 必须, 回测时间帧列表
# 可选值: "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d"
timeframes = ["1d"]
# 可选, 数据起始日期 (格式: "YYYY-MM-DD"), 默认: None (全部数据)
# start_date = "2024-01-01"
# 可选, 数据结束日期 (格式: "YYYY-MM-DD"), 默认: None (全部数据)
# end_date = "2024-12-31"

# ========================================
# 回测配置
# ========================================
[backtest]
# 必须, 初始资金
initial_balance = 10000.0

# ========================================
# 输出配置
# ========================================
[output]
# 可选, 输出目录, 默认: "reports"
dir = "/data/nutilustrader/reports/test"
# 可选, 是否导出逐笔交易详细数据, 默认: false
detail = false
# 可选, 是否覆盖已存在的回测结果, 默认: false (跳过已完成的组合)
overwrite = false

# ========================================
# 策略配置
# 每个 [[strategy]] 块定义一个策略
# type: 必须, 策略类型 (见下方列表)
# period: 必须, 参数范围
#   单值: period = 20
#   范围: period = { start = 1, end = 1000, step = 1 }
#
# 支持的策略类型:
#   sma, ema, dema, tema, hma, zlma, alma, fwma, jma,
#   kama, lin_reg, mc_ginley, pwma, sin_wma, swma,
#   tilson, trima, vidya, vwma, wilders, wma, ssf, ssf_st
# ========================================

[[strategy]]
type = "zlma"
period = { start = 1, end = 1000, step = 1 }

```

## 架构设计

```
main.rs # 只做入口, 调用 app::run (cli::parse → config::load_config → app::run)
lib.rs # 只做模块导入导出, `pub mod app/cli/error` + `#[path]` 挂载 app/NN_* 保持 config/data/engine/output/strategies 模块名
cli.rs # 命令行解析, clap 4(derive), 解析 --config/-c(必填, 无默认值)
error.rs # 统一错误类型, thiserror 2: AppError(Io/Config/Data/Parquet/Arrow/Msg) + Result, #[from] 自动转换
app/ # 执行代码
app/mod.rs # 整体逻辑编排 run(只做调度, 伪代码如下):
           #  1 加载配置并校验(deny_unknown_fields + validate); 构建策略 build_strategies → Vec<Box<dyn Strategy>>
           #  2 for symbol in symbols                    # 品种层
           #  3   for timeframe in timeframes           # 时间帧层
           #  4     cols ← load_columns(symbol, timeframe)  # 加载数据: 一次加载供该帧全部策略复用, 帧末释放; 失败打印[跳过]并 continue
           #  5     for strategy in strategies           # 策略层
           #  6       run_combo(...) → 单个组合处理 (见下)
           #  7 merge_parquet_files(combo_files) → result.parquet  # 只合并本次运行组合(新算+跳过), 每次整文件重建, 不扫描整个 temps/
app/mod.rs # 单组合处理 run_combo(symbol,timeframe,strategy):
           #  a 跳过判断: 非 overwrite 且 temps/{sym}_{tf}_{strat}_{参数指纹}.parquet 可读出 → 跳过(仍并入合并); 读失败则重算
           #  b 参数并行: (0..n).into_par_iter 单遍完成 信号生成 + 模拟(simulate / simulate_detailed)
           #  c detail 模式: 同遍收集 trades/equity_curve → detail_rows
           #  d 写出: 写 temps/ 组合文件(断点续跑依据); detail 模式即时按组合写 detail/
app/01_config.rs # 配置解析, toml + serde: Config{data/backtest/output/strategy}, 局部配置全量 deny_unknown_fields(PeriodParam untagged 除外), StrategyConfig(tag=type, 22 种), PeriodParam(单值|范围); Config::validate(资金>0 / symbol / timeframes / strategy 非空)
app/02_load_data.rs # K 线加载桥接, lib-reader DataSpec → KlineColumns(binance zip / dukascopy bi5)
app/03_run_backtest.rs # 持仓/成交/绩效引擎: 信号→全仓仓位→逐根 scan_trades(多空/复利/MAE-MFE)→compute_metrics(30+ 指标); simulate / simulate_detailed 供 run_combo 单参数调用
app/04_write_output/ # 输出
app/04_write_output/mod.rs # 再导出 parquet
app/04_write_output/parquet.rs # Parquet 读写: write_parquet(汇总) / write_single_parquet(单组合) / write_detail_parquet(逐笔详情) / read_parquet(缓存校验与合并) / merge_parquet_files(合并显式组合列表)
app/05_strategies/ # 策略叶
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals, Send+Sync) + Signal/bars_per_year 再导出
app/05_strategies/shared.rs # Signal 枚举 / price_cross_signal 穿越信号 / bars_per_year
app/05_strategies/build.rs # 策略构建工厂(与策略代码同聚合): build_ma! 宏 21 个 MA 变体 → Box<dyn Strategy>
app/05_strategies/{alma,dema,...,zlema}/… # 21 个单 MA 策略(周期遍历 → vector_ta 指标 → 价格穿越)
app/05_strategies/ssf/ # SSF 策略(supersmoother, param_str 含 variant=ssf)
app/05_strategies/ssf_st/ # SSF 平滑型(variant=ssf_st)
```
