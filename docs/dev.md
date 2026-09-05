# vta (vector-ta) 项目开发

## 需求分析

- 项目目标: 对多种技术分析指标 × 参数组合做向量化批量回测,一次遍历 `品种 × 时间帧 × (指标 × 参数范围)` 的笛卡尔积,按 SQN 排序输出终端表格与 Parquet 报告。

## 技术选型

- 语言: rust, edition = "2024"
- 指标计算: `vector-ta` 0.3(crates.io)
- 数据读取: 自研 `lib-reader`(workspace path 依赖,统一读取 binance/dukascopy K 线)
- 并行: rayon(信号生成与模拟均按参数并行)
- 输出: arrow 59 + parquet 59 + 终端表格
- 配置: clap 4(derive) + toml 1.1 + serde(derive)
- 错误: thiserror 2(`AppError`);入口仍用 anyhow 收敛
- 发布配置: `lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 架构设计

```
main.rs # 只做入口, 调用 app::run (cli::parse → config::load_config → app::run)
lib.rs # 只做模块导入导出, `pub mod app/cli/error` + `#[path]` 挂载 app/NN_* 保持 config/data/engine/output/strategies 模块名
cli.rs # 命令行解析, clap 4(derive), 解析 --config/-c(必填, 无默认值)
error.rs # 统一错误类型, thiserror 2: AppError(Io/Config/Data/Parquet/Arrow/Msg) + Result, #[from] 自动转换
app/ # 执行代码
app/mod.rs # 逻辑编排: build_strategies → 三层循环 symbol×timeframe×strategy(参数并行信号、temps 增量缓存) → 汇总 result.parquet / detail/
app/01_config.rs # 配置解析, toml + serde: Config{data/backtest/output/strategy}, StrategyConfig(tag=type, 22 种), PeriodParam(单值|范围)
app/02_load_data.rs # K 线加载桥接, lib-reader DataSpec → KlineColumns(binance zip / dukascopy bi5)
app/03_run_backtest.rs # 持仓/成交/绩效引擎: 信号→全仓仓位→逐根 scan_trades(多空/复利/MAE-MFE)→compute_metrics(30+ 指标, 批处理 rayon)
app/04_write_output/ # 输出
app/04_write_output/mod.rs # 再导出 parquet + table
app/04_write_output/parquet.rs # Parquet 读写(汇总/单组合/详情/增量读取合并)
app/04_write_output/table.rs # 终端表格, SQN 降序
app/05_strategies/ # 策略叶
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals) + Signal/bars_per_year 再导出
app/05_strategies/shared.rs # Signal 枚举 / price_cross_signal 穿越信号 / bars_per_year
app/05_strategies/build.rs # 策略构建工厂(与策略代码同聚合): build_ma! 宏 21 个 MA 变体 → Box<dyn Strategy>
app/05_strategies/{alma,dema,...,zlema}/… # 21 个单 MA 策略(周期遍历 → vector_ta 指标 → 价格穿越)
app/05_strategies/ssf/ # SSF 策略(supersmoother, param_str 含 variant=ssf)
app/05_strategies/ssf_st/ # SSF 平滑型(variant=ssf_st)
```