# vta (vector-ta) 项目开发

## 需求分析

- 项目目标: 对多种技术分析指标 × 参数组合做向量化批量回测,一次遍历 `品种 × 时间帧 × (指标 × 参数范围)` 的笛卡尔积,输出 Parquet 报告(`result.parquet` 合并结果 + 可选 `detail/` 逐笔交易/权益曲线)

## 技术选型

- 语言:rust
- edition = "2024"
- 指标计算: `vector-ta` 0.3(crates.io)
- 数据读取: 自研 `lib-reader`(workspace path 依赖,统一读取 binance/dukascopy K 线)
- 并行: rayon(单策略的参数索引按 `(0..n).into_par_iter` 并行,信号生成与回测同为一遍)
- 输出: arrow 59 + parquet 59
- 配置: clap 4(derive) + toml 1.1 + serde(derive)
- 错误: thiserror 2(`AppError`);入口 anyhow 收敛
- 发布配置: `lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 冒烟配置

```toml
[data]
type = "binance" # 必填, 数据源类型: "binance" | "dukascopy"
dir = "/data/nutilustrader/data/binance/data/um/daily/klines" # 必填, K线数据根目录
symbol = ["BTCUSDT"] # 必填, 回测品种列表
timeframes = ["1d"] # 必填, 可选 ["1m","3m","5m","15m","30m","1h","2h","4h","6h","8h","12h","1d"]
# start_date = "2024-01-01" # 可选, 数据起始日期, 格式 YYYY-MM-DD
# end_date = "2024-12-31" # 可选, 数据结束日期, 格式 YYYY-MM-DD

[backtest]
initial_balance = 10000.0 # 必填, 初始资金(>0)

[output]
dir = "/tmp/vta-smoke" # 可选, 输出目录, 默认 "reports"
detail = true # 可选, 是否导出逐笔交易/权益曲线, 默认 false
overwrite = true # 可选, 是否覆盖已存在结果, 默认 false(跳过已完成组合)

[[strategy]]
type = "sma" # 必填, 策略类型: sma,ema,dema,tema,hma,zlma,alma,fwma,jma,kama,lin_reg,mc_ginley,pwma,sin_wma,swma,tilson,trima,vidya,vwma,wilders,wma,ssf,ssf_st
period = { start = 5, end = 100, step = 5 } # 必填, 参数范围; 单值可写 period = 20
```

## 逻辑编排

- 启动 - main.rs 
  - 初始化日志
  - 解析 CLI - cli.rs - clap
  - 执行app业务 - app/mod.rs
    - 加载配置 - src/config.rs - toml + serde
    - 配置校验 config.validate()
    - 构建策略 build_strategies  # 每 [[strategy]] 块 → 1 个实例(内部 periods: Vec<usize>)
    - for symbol x timeframe     # 串行, 数据一帧加载一次、帧末释放
      - 加载数据 load_columns()  # 失败[跳过] continue; 供该帧全部策略复用
      - for strategy
        - 缓存跳过  # 非 overwrite 且 temps/{sym}_{tf}_{stg}_{参数指纹}.parquet 可读 → 跳过(仍并入合并)
        - 参数并行 (0..n).par_iter  # 每参数: 生成信号 + 回测 一波完成
        - 保存结果  # temps/ 每 sym x tf x stg 一文件; detail 模式另写 detail/ 每参数一文件
    - 合并 result.parquet  # 只合并本次 combo(新算+跳过), 整文件重建

```
启动 -> main.rs: cli::parse() 解析 --config/-c
v
加载配置 -> config::load_config: toml 反序列化 → Config(deny_unknown_fields) + validate(资金>0 / symbol / timeframes / strategy 非空)
v
构建策略 -> build_strategies(config.strategy): 21 个 MA 变体(build_ma! 宏) + ssf/ssf_st → Vec<Box<dyn Strategy>>
v
for symbol in symbols                        # 品种层(串行)
v
  for timeframe in timeframes               # 时间帧层(串行)
v
    load_columns(symbol, timeframe)          # lib-reader: DataSpec → KlineColumns; 失败打印[跳过]并 continue
v
    for strategy in strategies               # 策略层(串行)
v
      run_combo(strategy, symbol, tf, cols, ctx):
        a. 非 overwrite 且缓存 temps/{sym}_{tf}_{strat}_{参数指纹}.parquet 可读 → 跳过(仍并入合并)
        b. (0..n).into_par_iter 参数并行: 一波 生成信号 → simulate(/detailed)
        c. detail 模式收集 trades/equity_curve → detail_rows
        d. 写出 write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
v
合并 -> merge_parquet_files(combo_files) → {out_dir}/result.parquet   # 只合并本次运行组合(新算+跳过), 整文件重建
```

## 架构设计

```
# 支撑件(cli/error)在 crate 根, 不编号; 执行代码在 app/, 按"调用执行流程"编号: main → cli → 01_config → 02_load_data → 03_run_backtest → 04_write_output → 05_strategies
main.rs             # 入口: cli::parse() → config::load_config → app::run(&config); 错误 anyhow 收敛
lib.rs              # 只做导出: #[path] 挂载 app/NN_* 保持逻辑模块名(config/data/engine/output/strategies) + pub mod app/cli/error
cli.rs              # 支撑件: 命令行解析 clap 4(derive), 解析 --config/-c(必填, 无默认值), 返回 Cli
error.rs            # 支撑件: AppError(thiserror, Io/Config/Data/Parquet/Arrow/Msg), #[from] 实现 ? 自动转换
app/                            # 执行代码
app/mod.rs          # 整体编排 run(config) (伪代码):
                    #   1 config.validate()?; build_strategies → strategies
                    #   2 预算 out_dir (缺省 "reports") → temps_dir/detail_dir; 解析 detail/overwrite/balance
                    #   3 for symbol × timeframe         # 品种×时间帧(串行)
                    #   4   load_columns → cols; 失败[跳过] continue
                    #   5   bpy = bars_per_year(tf); 构建 ComboCtx
                    #   6   for strategy: run_combo → (path, computed); 计 ran/skipped
                    #   7 merge_parquet_files(combo_files) → result.parquet
app/mod.rs          # 单组合处理 run_combo(strategy, symbol, tf, cols, ctx):
                    #   a 缓存跳过: 非 overwrite 且组合文件(参数指纹)可读 → 跳过
                    #   b (0..n).into_par_iter 并发生成信号+模拟; detail 收集 trades/equity_curve
                    #   c 写 temps/ 组合文件 [+ detail/]; 返回 (path, computed)
app/01_config.rs    # 配置解析, toml + serde: Config{data/backtest/output/strategy}, 全量 deny_unknown_fields(PeriodParam untagged 除外); StrategyConfig(tag=type, 22 种); PeriodParam(单值|范围); Config::validate(资金>0 / symbol / timeframes / strategy 非空)
app/02_load_data.rs # K 线加载桥接: DataConfig + symbol/tf → lib-reader DataSpec → KlineColumns(binance zip / dukascopy bi5)
app/03_run_backtest.rs # 回测引擎: 持仓/成交/绩效 — summary 指标(30 项)/Trade/SimResult; simulate / simulate_detailed 供 run_combo 单参数调用
app/04_write_output/    # 输出
app/04_write_output/mod.rs # 再导出 parquet 模块
app/04_write_output/parquet.rs # Parquet 读写: write_single_parquet(单组合 temps/) / write_detail_parquet(detail/) / read_parquet(缓存校验) / merge_parquet_files(显式合并)
app/05_strategies/      # 策略叶
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals, Send+Sync) + shared 再导出
app/05_strategies/shared.rs # Signal 枚举(Hold/Buy/Sell/Close) / price_cross_signal 穿越信号 / bars_per_year
app/05_strategies/build.rs # 策略构建工厂(build_ma! 宏 21 个 MA 变体 → Box<dyn Strategy>) + ssf/ssf_st
app/05_strategies/{alma,dema,...,zlema}/… # 21 个单 MA 策略(周期遍历 → vector_ta 指标 → 价格穿越)
app/05_strategies/ssf/  # SSF 策略(supersmoother, param_str 含 variant=ssf)
app/05_strategies/ssf_st/ # SSF 平滑型(variant=ssf_st)
```

## PBO 模式(判局用 C3)

```bash
# 1. 先用 detail=true 跑出逐笔 fixed 曲线(detail/ 下 *_fixedcurve.parquet)
# 2. 按(币,周期,策略)分组实算 CSCV(Bailey et al. 2016, S=16→12870 logits)
cargo run --release -- --pbo /path/to/reports_XXX
```

- 与 `-c` 二选一; T×N 常驻内存, 1m×500 级约 24GB——PBO 建议 TF≥30m 或小 N。
- T<32 或 N<10 的组跳过并明示; φ>0.05 按 Neyman-Pearson 惯例拒绝(C3)。
- 源码: `src/pbo.rs`(纯 CSCV+单测: 噪声φ高/植入边际φ低) + detail fixedcurve 写读。
