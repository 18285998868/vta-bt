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
- Rust 版本：**始终使用最新 stable**（`rust-toolchain.toml` 的 `channel = "stable"`，无需人工跟进版本号；刻意不声明 `rust-version` 下限，以免 `clippy::incompatible_msrv` 拦下新 API）
- 并行：rayon（参数索引 `into_par_iter`）
- 输出：arrow 60 + parquet 60
- 配置：clap 4（derive）+ toml 1.1 + serde（derive）
- 错误：thiserror 2（`AppError`）；入口 anyhow 收敛
- 发布配置：`lto = "fat"`、`codegen-units = 1`、`strip = "symbols"`

## 冒烟配置

```toml
[data]
type = "binance" # 必填，数据源类型："binance" | "dukascopy"
market = "futures/um" # 可选，Binance 市场："spot" | "futures/um" | "futures/cm"，默认 futures/um
dir = "/data/nutilustrader/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = ["BTCUSDT"] # 必填，回测品种列表
timeframe = ["1d"] # 必填(单数!), 可选 ["1m","3m","5m","15m","30m","1h","2h","4h","6h","8h","12h","1d","1w"]
# 也可写单值: timeframe = "1d"
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

## 关键设计约束（改动前必读，18 条）

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
| `Ratio` | **小数**比率 | `total_return`、`max_drawdown`、`win_rate`、`expectancy`、`fixed_total_return`、`fixed_max_drawdown`、`buy_hold_return` |
| `Percent` | 百分数（×100） | `volatility_pct` |
| `Money` | 以初始资金计价的货币 | `balance`、`final_equity`、`avg_win`、`avg_loss`、`fixed_pnl`、`buy_hold_pnl` |
| `Price` | 价格差（报价货币，**非金额**） | `avg_mae_price`、`avg_mfe_price` |
| `Bars` / `Count` | bar 数 / 整数计数 | `avg_hold_bars` / `trades`、`bars_used`、`delay_bars`、`consecutive_*` |
| `PerYear` | 次 / 年 | `turnover_per_year` |
| `BasisPoints` | 基点 | `fee_bps`、`slippage_bps`、`fixed_expectancy_bps` |
| `Dimensionless` | 无量纲非比率 | `sharpe`、`sortino`、`profit_factor`、`calmar`、`skewness`、`kurtosis`、`p_value` … |

**已修正的单位错配**：`max_drawdown` 一直是小数，而 `fixed_max_drawdown_pct`
原本也是百分数 —— 两条腿并排对比时 fixed 腿会凭空大 100 倍。现已统一为小数并改名
`fixed_max_drawdown`（同时删掉了名字里的 `_pct`）。

**毛/净口径对照**（同一批交易，差别只在佣金与汇总方式）：

| 列 | 口径 | 汇总方式 | 用途 |
|---|---|---|---|
| `expectancy` | **毛**：含滑点（成交价已 slip）、**不含佣金** | 逐笔 `pnl_pct` 算术平均（比率） | 判断信号边际，与费率配置解耦 |
| `fixed_expectancy_bps` | **净**：含滑点 + **双边佣金** | 固定初始名义的逐笔净盈亏均值（bp） | 判断扣费后还剩多少；与 `expectancy×1e4` 的差即成本拖累（≈ 两倍单边费率） |
| `total_return` | 净、**复利** | 按当前权益全押的链式收益 | 实际资金曲线业绩 |

`fixed_expectancy_bps × n` 是**算术加总**，可能低于 −100%（实测某组为 −155%），
不代表可实现的亏损 —— 加总假设每笔都押初始资金；链式口径的下限是 −100%，
两者之差是波动率拖累与破产下限的合计。

`calmar = CAGR / max_drawdown`，`recovery_factor = total_return / max_drawdown`。
旧列 `return_mdd` 把后者又算了一遍（`total_return_pct / max_drawdown_pct`），
与 `recovery_factor` 完全重复，已从 schema 删除。

新增列若不在 `COLUMNS` 表里声明单位，`test_every_report_column_declares_its_unit`
会失败。

### 5b. 退化结果必须可被识别：`bars_used` + `status`

> `status` 有 5 个取值：`ok` / `no_signals` / `no_trades` / **`invalid`** / **`bankrupt`**。
> `invalid` = **该参数档位不可计算**（数据/参数不足，如 `period` 超过可用 bar 数），
> 与"跑了但没信号"**必须**区分 —— 对参数网格过拟合研究，前者是最该被发现的
> 失败模式，绝不能与"零波动低风险"混为一谈。详见「设计决策复核」D1。
> `bankrupt` = 权益（现金）已降到 ≤ 0，引擎拒绝再开新仓；它**跑出来了**，
> 但收益率已失去可比性。`invalid` / `bankrupt` 都不得进入胜率分母或 PBO。

指标全 0 有两种截然不同的成因：**真的零波动低风险**，或**策略根本没跑起来**
（数据长度 < 策略窗口 → 全程 Hold）。实测反例：同一 `params` 串 `period=400`

- 1d / 300bar → `trades=0 / pnl=0 / sharpe=0`
- 1m / 20000bar → `trades=1310 / pnl=-1.0 / sharpe=-18.04`

`params` 只描述参数、不描述数据规模，两者无法区分。报告因此带两列：

- `bars_used`：本次求值实际喂给策略的 bar 数
- `status`：`ok` / `no_signals`（全程 Hold）/ `no_trades`（有信号但一笔未成交）/ `invalid`（该档不可计算）/ `bankrupt`（权益 ≤ 0）

**读表时必须按 `status == "ok"` 过滤，并结合 `bars_used` 判断样本量。**
`no_signals` 同时覆盖"窗口 > 数据长度"与"该参数本就无信号"两种成因（引擎层
不可区分），`bars_used` 提供区分所需信息。

### 6. 参数网格的唯一性在构建期校验（字符串层 **与** 编码层）

`strategy_params` 整数列由 `strategy_params_value` 把参数元组压进 `u64`，未知形态回退 `fnv1a_32`（**有损**）。
行主键实为 `(symbol, timeframe, strategy, params)` 四元组，该列仅作"同策略内按参数大小排序"的代理，碰撞不构成数据损坏，但会让排序错乱。

`build_strategies_checked` 在每次启动时对展开后的完整网格做**两层** `HashSet` 自检，任一层碰撞即**报错退出**（O(N)，微秒级）：

1. **字符串层**：`param_str(i)` 两两不同 → 捕获"网格展开有误 / `param_str` 未覆盖全部维度"。
2. **编码层**：`strategy_params_value(param_str(i))` 两两不同 → 捕获真正会**塌陷**的那一层。

> **只查字符串是不够的。** 二者是不同映射：字符串可以两两不同，压缩后的整数却撞在一起，
> 而报告排序/去重用的正是整数列。

**历史缺陷（已修）**：编码器的兜底分支对**任何**未识别形态一律"取末尾数字"。对
`edma_p5_o0` / `sgma_p10_m2` / `atma_u5_d10` / `macd_12_26_5` 这类含**多个下划线**的串，
末尾数字只编码了最后一个维度，于是整族塌陷成一个值：

| 策略 | 塌陷前 | 后果 |
|---|---|---|
| `edma` | `edma_p1_o0` … `edma_p500_o0` 全为 `0` | 500 组参数同值，排序完全失效 |
| `sgma` | `sgma_p1_m2` … `sgma_p500_m2` 全为 `2` | 同上 |
| `atma` | 20 组各自塌陷 | 同上 |

同一根因也让 `params_to_json` 谎报参数：`edma_p5_o0` 曾被写成 `{"period":"0"}`，
而它真实的 period 是 **5**（0 是 offset）—— 下游按 period 分组会直接归错类。

**修复后的规则**：

- 编码：仅当串严格形如 `<前缀>_<数字>`（前缀不含下划线，即 `sma_10`）才取末尾数字；
  其余形态一律 FNV（整串哈希）。`sma_10` / `ema_3` / `t3_12` 的封存口径**逐位不变**。
- JSON：多参数形态按参数串里原有的键名逐个还原（`edma_p5_o0` → `{"p":"5","o":"0"}`），
  不再谎称 `period`；单参数形态（`sma_10`）仍是 `{"period":"10"}`。

新增策略后若看到该报错，说明 `param_str` 格式写错了（或该策略的编码分支需要补）。

### 7. 缓存指纹覆盖数据身份**与产出文件集**

`temps/{...}_{指纹}.parquet` 的指纹 = FNV-1a(**行情内容哈希** + 资金 + 日期窗口 + 执行口径 + **detail / fixed_curves 开关** + **实现版本(源码内容哈希)** + 逐参数 `param_str`)。

**实现版本这一维是必需的**：改完策略/指标代码后重跑，若指纹不变则**全部命中旧缓存** ——
报告里是修复前的结果，而使用者以为拿到了新结果，且连日志都不会提到缓存命中。
实现方式是 `build.rs` 对本 crate 与两个 path 依赖的 `src/` 做内容哈希
（用内容哈希而非 `git rev-parse`：monorepo 里改一个无关的兄弟 crate 不该让缓存
全量失效；非 git 检出时 `git` 不可用；**工作区有未提交改动时 commit hash 不变** ——
恰恰是"刚改完代码"这个最需要失效的时刻不会失效）。
另提供 `--no-cache` 逃生门。

**产出开关为什么必须在指纹里**：缓存命中会**直接早退**，根本走不到 detail 分支 ——
而 detail 产物（逐笔交易 / 权益曲线 / fixed 曲线）只由该分支写出。开关不进指纹时，
"先 `detail=false` 跑一次、再 `detail=true` 跑第二次"会**全部命中缓存**，
`detail/` 目录一个文件都没有，而用户以为自己拿到了 detail 数据；紧接着
`vta --pbo` 报"缺少 detail 索引，请先用 `detail=true` 且 `fixed_curves=true` 重跑" —— 而用户
**确实**设了 `detail=true`。死循环。指纹的定义即"这次要产出什么"，产出集不同 ⇒
身份不同 ⇒ 必然重算。回归测试 `fingerprint_covers_output_set_switches`。

**指纹必须跨机器可复现**：整数列用 `to_le_bytes()`（小端）而非 `to_ne_bytes()`
（本机字节序）—— 否则同一份数据在 x86 与 ARM 上得到不同指纹，缓存无法在异构机器间复用。
回归测试 `fingerprint_uses_little_endian_byte_order`。

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

**键名必须诚实**：多参数形态（`edma_p5_o0`）按串里原有的键名还原为 `{"p":"5","o":"0"}`，
**不得**把最后一个维度伪装成 `period`（详见第 6 条）。

### 9. `read_parquet` 按**列名**索引，禁用魔法列号

`ColumnIndex` 把列名映射成下标：必需列缺失即**报错并列出实际列名**，可选列
（旧缓存没有的追加列）按名字查、查不到才回退。

历史实现用常量列号（`f64_col(&batch, 11)`），且用 `num_columns() > 37` 判断列存在
—— 每追加一列，后续列号全部偏移，而相邻同为 `Float64` 的列 `downcast_ref`
**不会 panic，而是静默读错列**（例如把 `sharpe` 读成 `sortino`）。

新增输出列仍应**追加到末尾**以兼容旧缓存，但正确性不再依赖这一点。

**曲线读取同理**：`read_equity_curves` 与 `read_fixed_curves` 都用 `col_by_name`
取 `time`/`equity` 与 `fixed_equity`，**不得**改回下标 `1`（两条曲线列语义不同，
读错列的类型恰好兼容，不报错）。二者刻意**不复用同一个 reader**。

### 10. 缓存行数也是有效性的一部分

指纹相同**不保证**文件完整（可能被截断、被旧版本写入）。`run_combo` 要求缓存
行数等于 `strategy.param_count()`，不符即判失效重算。

**行数是轻量校验，不覆盖 schema**：命中路径只读 footer 行数、不解码列。若报告
schema 发生增删列或改类型，旧缓存要到 merge 阶段的 `read_parquet`（按列名取值）
才会报"必需列缺失"。**改 schema 后必须删 `temps/` 或加 `--no-cache`。**

**detail 模式还要求整组产物完整**：每个组合文件旁有 `{stem}.detail.toml`，记录
本次写出的 trades / equitycurve / fixedcurve 文件名与字节数。缓存命中时逐项校验
文件存在、大小未变、schema metadata 的 `combo` 属于该组合；任一不符就重算该组合
（而不是只补写"看起来缺"的一个文件）。这解决了"手动删 `detail/` 后全部命中缓存、
一个 detail 文件都不补"以及"同名文件被另一轮数据窗口覆盖"两类静默错误。

### 11. `RunResult` 只有一个构造点

detail 与非 detail 两条分支共用 `ComboCtx::run_row`。此前两份 16 字段字面量
各自维护，任何新增字段都可能只改到其中一条 —— 该漂移**已经真实发生过**
（非 detail 分支曾内联写一份字面量）。

回归测试 `both_combo_branches_construct_run_rows_via_run_row` 用源码级断言钉死：
`run_combo` 函数体内不得出现 `RunResult {` 字面量，且 `ctx.run_row(` 恰出现 2 次。

### 12. 指标与 `--detail` 开关无关（类型级保证）

`simulate` 与 `simulate_detailed` 的指标都只经**同一条**在线累加器
（`MetricsSource`）计算。这不只是"测出来一致"，而是**类型上不可能不一致**：
`compute_metrics` 的签名收 `&MetricsSource`，而 `MetricsSource` 里**没有**
`trades` / 曲线字段 —— 指标在编译期就看不到 `--detail` 的落盘产物。

回归测试 `non_detailed_metrics_match_detailed_bit_for_bit` 锁定逐位一致，
`test_simulate_does_not_materialize_equity_curve` 锁定非详细路径不物化产物。

### 13. 引擎不做无谓分配

- `CurveAccum`：`simulate()` 只算统计量，**不物化权益曲线**（曲线部分 O(1)）。
  注意逐笔 pnl 统计序列仍需保留（O(交易笔数)，最坏 O(T)）用于 SQN/偏度/峰度/
  分半 PF；这仍远小于"每条 bar 两条 f64 曲线"的物化成本。与"物化后单遍遍历"
  **逐位一致**，由
  `test_curve_accum_is_bit_identical_to_materialized_reference` 锁定。
- `CloseOutcome`/`TradeAccum`：非 detailed 路径不构造 `Vec<Trade>`，
  `exit_reason` 是 `&'static str`（原为每笔一个堆 `String`）。
  两条路径的指标必须逐位一致，由 `non_detailed_metrics_match_detailed_bit_for_bit`
  锁定。
- `Side` enum 取代 `i8` 方向：原实现里 `side == 2` 会静默走"卖出"分支。

### 14. K 线六列必须等长

生产路径由 `assert_columns_aligned` 在进入回测前一次性拒绝六列不等长，**不报错
地少算几根 bar** 不是可接受行为。

引擎 `scan_trades` 的循环上界只用 `close.len().min(signals.len())`，**不再**把
`open.len()` 算进去：旧实现下 `open = []` 会让整段信号被静默丢弃。`open` 短于
`close` 时，缺失的开盘价由 `fill_at` 退回收盘价（公共 API 的宽松契约）；`time`
短于 `close` 时 `ts(i)` 退化为 bar 序号，仅影响 detail 的时间列。

**策略层也必须自己取四列的最小长度**：`vwap::session_vwap` 曾用 `close.len()`
当循环上界却索引 `high[i]`/`low[i]`/`volume[i]` —— 生产路径由上面的等长校验兜住，
但它是 crate 内的 `pub` 面，不该替调用方承担这个前提。兄弟策略
`vwap_trend::session_vwap` 用的就是 `min()` 口径。

### 15. 钱不用 `f64` 承载

货币金额一律走 `money::Amount`（`rust_decimal::Decimal` 定点十进制）：
`InitialBalance`、引擎钱账本（`cash` / 持仓数量 / 逐笔盈亏 / 累计已实现盈亏）、
`SummaryMetrics` 的货币字段、报告与 detail 的货币列（Parquet `Decimal128(38, 8)`）。

**为什么**：`f64` 只有 53 位有效数字，且**能表达** `NaN`/`Inf` —— 金额为 `NaN` 不会被
任何比较运算捕获，会一路污染权益曲线与全部指标；且大额多次累加会累积舍入误差。
`Decimal` 是 128 位定点数，金额运算**精确**，且**无法**表示 `NaN`/`Inf`。

**边界（刻意仍是 `f64`）**：价格（要参与 `ln`/`sqrt`）、比率（无量纲）、
权益曲线**序列**（供 `CurveAccum` 与 PBO 的 CSCV 用）。逐条写在 `src/money.rs`。

detail trades 的金额列是 `qty` / `pnl` / `fixed_pnl`。旧列名 `total_return`
存的其实是**单笔盈亏金额**，与报告里的同名比率列 `total_return` 同名不同量纲，
已改名为 `pnl`。

**代价**：报告与 detail 的**历史文件不兼容**（金额列类型变了）。

### 16. 信号只在 fill bar 改变持仓与权益（不得在信号 bar 提前记账）

`scan_trades` 用 `VecDeque<Side>` 把每个信号 bar 算出的目标方向排队，延迟
`delay_bars` 根 bar 后才在**到期 bar** 改 `cur_pos` / `cash` / 开平仓。
bar `i` 的权益只由 bar `i` 及之前**已经发生**的成交决定。

历史缺陷：旧实现在信号 bar `i` 就改持仓与现金，却用 `i + delay` 的
开盘/收盘价成交。于是 bar `i` 的权益按**未来成交价**盯市 —— 例如 bar0 信号
Buy、bar1 开盘从 100 跳到 200 时，bar0 权益被记成 500 而非 1000，零收益策略
甚至能算出 `sharpe = 11.22`。Sharpe / 波动率 / 最大回撤 / Sortino 全部失真，
只有最终总收益碰巧不变。

回归测试 `equity_curve_does_not_mark_to_future_fill_price`。成交价、交易笔数与
MFE/MAE 的取值区间与旧口径一致；变化的是**曲线时点**与由它派生的指标。

### 17. detail 产物可追溯，PBO 只比较同网格的 `ok` 曲线

- 每个组合文件旁有侧车 `{stem}.detail.toml`，记录本次写出的 trades /
  equitycurve / fixedcurve 文件名与字节数。缓存命中时逐项校验存在、大小、
  文件 metadata 的 `combo`；任一不符即重算该组合。
- detail 每条曲线/交易文件的 schema metadata 带 `combo`（产出组合）与 `status`
  （该档位状态）。fixedcurve 用 `time`（毫秒）而非 `bar_idx`，PBO 才能确认两条
  曲线共享**同一时间网格**。
- `detail/manifest.toml` 是 `--pbo` 的唯一入口：每轮回测开始时删除，结束时按本轮
  组合重写，只列 fixed 曲线；每条记录带文件名、`combo` 与字节数。`--pbo` 不再扫
  目录 —— 参数已删除或上一轮遗留的曲线不会静默参与 CSCV；文件被截断/替换时
  索引校验会直接报错。
- PBO 只取 `status == ok`、时间列严格递增且与值等长的曲线，按精确时间网格分组，
  选样本数最大的 cohort；不再按 `min(len)` 截断。
- 索引/文件缺失或 `combo` 不一致时**报错并要求重跑**，不得少读几条继续算 φ。

## 逻辑编排

```
启动 -> main.rs: cli::parse() 解析 --config/-c（或 --pbo <reports_dir>，二选一）
v
加载配置 -> config::load_config：toml 反序列化 → Config(deny_unknown_fields) + validate(资金>0 / symbol / timeframe 非空 / 周期合法 / 各策略参数范围)
v
构建策略 -> build_strategies_checked(config.strategy)：单个穷尽 match 覆盖全部 85 个变体 + 参数网格两层唯一性自检(字符串/编码) -> Vec<Box<dyn Strategy>>
v
for symbol in symbols                        # 品种层（串行）
v
  for timeframe in timeframes               # 时间帧层（串行）
v
    load_columns(symbol, timeframe)          # kline-reader：dir 为根 → KlineColumns；失败打印[跳过]并 continue
v
    for strategy in strategies               # 策略层（串行）
v
      data_fp = data_fingerprint(cols)   # 策略层循环**外**算一次, 供全部策略复用
      run_combo(strategy, symbol, tf, cols, bpy, data_fp, ctx):
        a. 非 overwrite 且缓存 temps/{sym}_{tf}_{strat}_{参数指纹}.parquet 可读
           [+ detail 模式: 侧车清单/文件大小/combo 元数据全部校验通过] → 跳过（仍并入合并）
        b. (0..n).into_par_iter 参数并行：一波 生成信号 → simulate(/detailed)
        c. detail 模式收集 trades/equity_curve → detail_rows
        d. 写出 write_single_parquet(temps/) [+ write_detail_parquet(detail/) + 侧车清单]
v
合并 -> merge_parquet_files(combo_files) → {out_dir}/result.parquet   # 只合并本次运行组合（新算+跳过），整文件重建
detail 模式 -> write_detail_index(detail/manifest.toml)                # 只列本轮 fixed 曲线
```

## 架构设计

```
# 支撑件(cli/error/pbo)在 crate 根，不编号；执行代码在 app/，按"调用执行流程"编号：main → cli → 01_config → 02_load_data → 03_run_backtest → 04_write_output → 05_strategies
main.rs             # 入口：cli::parse() → -c 走 config::load_config → app::run(&config)，--pbo 走 pbo::run_pbo_report；二选一，错误 anyhow 收敛
lib.rs              # 只做导出：#[path] 挂载 app/NN_* 保持逻辑模块名(config/data/engine/output/strategies) + pub mod app/cli/error
cli.rs              # 支撑件：命令行解析 clap 4(derive)，解析 --config/-c + --pbo（二选一），返回 Cli
config.rs           # 支撑件：Config{data/backtest/output/strategy} + DataSection::for_pair(symbol, tf) 降为 kline-reader 单值 DataConfig
error.rs            # 支撑件：AppError(thiserror, Io/Config/Data/Parquet/Arrow/Msg)，#[from] 实现 ? 自动转换
pbo.rs              # 支撑件：CSCV 实算（Bailey et al. 2016），T×N 常驻内存
app/                            # 执行代码
app/mod.rs          # 整体编排 run(config)：validate → build_strategies_checked → 预算 out_dir → for symbol × timeframe(串行) → load_columns → data_fp(循环外算一次) → for strategy: run_combo → merge → result.parquet
app/03_run_backtest.rs # 回测引擎（lib 映射为 engine）：持仓/成交/绩效 — SummaryMetrics(30 项)/Trade/SimResult；simulate / simulate_detailed 供 run_combo 单参数调用
app/04_write_output/    # 输出（lib 映射为 output）
app/04_write_output/parquet.rs # Parquet 读写：write_single_parquet(单组合 temps/) / write_detail_parquet(detail/) / read_parquet(缓存校验) / merge_parquet_files(显式合并)
app/05_strategies/      # 策略叶（lib 映射为 strategies，~80+ 子目录）
app/05_strategies/mod.rs # Strategy trait(name/param_count/param_str/generate_signals_full/generate_signals_batch, Send+Sync) + shared 再导出
app/05_strategies/shared.rs # Signal 枚举(Hold/Buy/Sell/Close) / price_cross_signal 穿越信号 / bars_per_year
app/05_strategies/build.rs # 策略构建工厂：build_one 单个穷尽 match(全部 85 变体, 无 `_` 兜底) → Box<dyn Strategy>；build_jobs_checked 保留每条 stop 并加两层唯一性自检
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

语义层：`initial_balance > 0`；`symbol` / `timeframe` / `strategy` 非空。

#### 数据加载（dir 为根）

`DataSection::for_pair(symbol, tf)` 降为 kline-reader 单值 `DataConfig`（`dir` 原样透传，kline-reader 自动追加数据源子路径）→ `load_columns`；失败打印 `[跳过]` 并 `continue`，供该帧全部策略复用，帧末释放。

#### run_combo（缓存 + 参数并行 + 写盘）

`params_fingerprint()`（FNV-1a 混入 balance/window/delay/fee/slip/fill + **detail/fixed_curves 开关** + 各 param_str）定位 `temps/{sym}_{tf}_{stg}_{fp:016x}.parquet`：`overwrite = false` 且可读即跳过；`detail` 模式还要求侧车 `{stem}.detail.toml` 及全部产物存在、大小/combo 元数据一致，否则重算该组合。否则 `(0..n).into_par_iter` 生成信号+模拟一波完成；`detail` 模式按 `DETAIL_MEM_BUDGET = 4GiB` 分批（`per_param = len×200B`）防爆内存。产出集开关进指纹的理由见「关键设计约束」第 7 条。

#### 合并

`merge_parquet_files(combo_files)` 只合并本次组合（新算 + 跳过），整文件重建 `result.parquet`。

## PBO 模式

> **分块笔数必须与累加映射同源**（`pbo::block_counts`）：CSCV 把逐 bar 对数收益
> 按 `b = i·S/(T−1)` 分块累加，而 IS/OOS Sharpe 的**分母**（笔数）必须用**同一套
> 边界**算出来 —— 即 `count(b) = ceil((b+1)·m/S) − ceil(b·m/S)`（`m = T−1`，
> **上取整**）。用整除差（floor 边界）在 `S ≥ 2` 时**永远不等**（`T` 是 `S` 的倍数
> ⇒ `S ∤ (T−1)`），于是均值被放大/缩小、方差自由度同步错；`cnt` 被低估时
> `sq − sum²/cnt` 可能变负 ⇒ 方差被夹到 0 ⇒ **IS Sharpe 被静默强制为 0**
> （低波动策略最易中招）⇒ `r_is`/`r_oos` 的 argmax 改变 ⇒ **φ 被算错**。
> 回归测试 `block_counts_match_brute_force_accumulation`（对照暴力计数）
> 与 `cscv_matches_materialized_reference`（对照物化参照实现，φ 逐位一致）。

```bash
# 1. 先用 detail=true + fixed_curves=true 跑一轮；结束时写 detail/manifest.toml
# 2. 按（币，周期，策略）分组实算 CSCV（Bailey et al. 2016，S=16→12870 logits）
cargo run --release -- --pbo /path/to/reports_XXX
```

- 与 `-c` 二选一；T×N 常驻内存，1m×500 级约 24GB——PBO 建议 TF≥30m 或小 N。
- **只读 `detail/manifest.toml` 索引列出的 fixed 曲线，不再扫目录**；索引缺失/过期
  会直接报错要求重跑，避免上一轮遗留或参数已删除的曲线静默参与 CSCV。
- 只使用 `status == ok` 的曲线；`no_signals` / `no_trades` / `bankrupt` 显式筛掉并
  `warn!` 条数。时间列必须严格递增；按**精确时间网格**分组，选样本数最大的 cohort，
  不再按 `min(len)` 截断（旧实现会让一条短曲线把整组 T 拉小且无提示）。
- T<32 或 N<10 的组跳过并明示；φ>0.05 按 Neyman-Pearson 惯例拒绝。
- 源码：`src/pbo.rs`（纯 CSCV + 单测：噪声φ高/植入边际φ低）+ detail fixedcurve 写读。

## 问题

- 当 `dir` 写成内层 klines 路径时，报目录不存在，
  - 解决：`dir` 统一为**数据根**（如 `/data/nutilustrader/data`），kline-reader 自动追加子路径；不要手写 `binance/...` 进去
- 当读取 spot / CM 数据却未设置 `market` 时，会按默认 `futures/um` 查找目录，
  - 解决：显式设置 `market = "spot"` 或 `market = "futures/cm"`；`futures/um` 可省略
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

## 2026-10 代码审查修复（rust-skills）

本轮审查发现并修复的问题，按严重度排列。完整回归测试见 `tests/`。

### 正确性（静默出错类）

- **`build_strategies_checked` 从未被调用**（Critical）
  - `run()` 曾直接调 `build_strategies`，而带唯一性自检的 checked 变体全仓库零调用方
    —— 本文档此前声称的"每次启动时自检"**与代码事实不符**。
  - 现在 `run()` 走 checked 变体；回归测试 `run_uses_the_uniqueness_checked_strategy_builder`。
- **`RunResult` 出现第二个构造点**（Critical）
  - 非 detail 分支内联写了一份 16 字段字面量，正是本文件第 11 条声称已消除的漂移。
  - 两分支统一走 `ComboCtx::run_row`，顺带把同一行里重复 3 次的 `param_str(i)`（每次分配
    一个 `String`）收敛为 1 次。
- **`strategy_params` 编码塌陷**（Critical，见第 6 条）
  - `edma` 500 组 / `sgma` 500 组 / `atma` 400 组参数全部编码为同一个值，排序代理完全失效。
  - 同一根因让 `params_to_json` 把 `edma_p5_o0` 写成 `{"period":"0"}`（真实 period 是 5）。
- **`partial_cmp().unwrap()` 遇 NaN panic**（High）
  - PBO 的两处 argmax 改用 `f64::total_cmp`（全序、无 `None`）。
- **`read_fixed_curves` 用魔法列号 `1`**（Medium）
  - 与本项目自己写下的"按列名索引"规则冲突；改为 `col_by_name("fixed_equity")`。

### 鲁棒性

- **生产路径的 `expect("non-UTF-8 path")`**（Medium，5 处）
  - 输出目录来自配置，用户填非 UTF-8 路径时整轮回测在第一个组合 panic。
  - `write_parquet` / `read_parquet` / `read_*_curves` / `merge_parquet_files` 全部改收 `&Path`；
    `OutputConfig.dir`、`ComboCtx.temps_dir/detail_dir`、`ColumnIndex.file` 改 `PathBuf`。
- **`eprintln!` 绕过 tracing**（Medium）
  - 合并阶段的读失败告警改 `tracing::warn!`，可被 `RUST_LOG` 过滤。

### 性能

- **`data_fingerprint` 按策略重算**（High）
  - 该函数遍历六列逐字节 FNV（自述 ~20ms/1M bars），原先在 `run_combo` 内调用，
    而 `run_combo` 按策略逐个调用 ⇒ **同一份行情被哈希 N 次**（100+ 策略即 100 倍浪费）。
  - 现在在 `run()` 的策略循环**外**算一次，以参数传入。零语义变化。
  - 回归测试 `data_fingerprint_is_hoisted_out_of_the_strategy_loop`。

### 架构

- **策略构建的三处手工同步清单收敛为一处**（P5）
  - 原先 `build_ma!` 宏的 `unreachable!()` 兜底、`matches!` 排除表、60 条 `filter_map` 分支
    需同步维护；漏改前两处**不报错**，只会让该策略被静默跳过（配置写了却不跑）。
  - 现在是单个**穷尽 match**（`build_one`，无 `_` 兜底），编译器强制覆盖全部 85 个变体。
    回归测试 `every_variant_has_a_construction_arm` / `real_configs_build_all_their_strategies`。
- **指标与落盘产物在类型上分离**（Q2）
  - `ScanOut` 拆成 `MetricsSource`（指标唯一来源）与 `DetailArtifacts`（仅 detailed）。
    `compute_metrics` 签名收 `&MetricsSource`，故指标**编译期就看不到** `--detail` 的产物。
- **补充 `[profile.bench]`**（P4）
  - 刻意**不设** `panic = "abort"`：测试用 `catch_unwind` 断言 `from_bps(0)` 会 panic。

### 文档与 CI

- 公开 API 的 `missing_docs` 从 **648 → 0**，`lib.rs` 补 crate 级文档与三条核心不变量。
- CI 新增 `cargo doc -D missing_docs` 门禁，防止新增项无文档混入。

## 2026-10 代码审查修复（rust-skills）· 第二轮

第一轮见上一节。本轮针对**静默出错**与**资源安全**两条主线，共 21 项；
另按确认结果实施了 7 项**设计决策**（D1–D7，见「设计决策复核」一节）。

每项都配了回归测试；策略层重构（41 个 config 收敛到单一 `grid_prealloc`、
3 份 `pct_b` 收敛到 `shared::pct_b`）经**逐字节比对**验证未改变任何回测数值。

### 设计决策的落地（D1–D7）

- **D1** 66 个策略的 95 处「静默降级为全 Hold」→ 显式 `ParamOutcome::Skipped`
  + 报告 `status = "invalid"`，**不中断**整轮。
- **D2** 新增 `build.rs`（源码内容哈希进指纹）+ `--no-cache` 逃生门。
- **D3** 16 处硬编码的 BB 周期 → `bblen` 参数（缺省 20，数值不变）。
- **D4** 7 个策略的 11 处**盘中止损** → 统一为收盘价判定（这些策略历史结果失效）。
- **D5** 30+ 个 bar 数参数 `F64Param` → `PeriodParam(usize)`（小数被拒）。
- **D6** `symbol`/`timeframe` 白名单 + detail 文件名加哈希（消除路径穿越与撞名）。
- **D7** 新增 `src/money.rs` 的 `Amount`（`rust_decimal` 定点十进制）：
  钱的账本、指标、报告与 detail 的货币列（`Decimal128(38,8)`）全部脱离 `f64`。

### 正确性（静默出错类）

- **CSCV 分块笔数与实际累加不一致 ⇒ φ 被算错**（Critical）
  - 收益按 `b = i·S/(T−1)` 分块累加，但 Sharpe 的分母笔数用 `b·m/S..(b+1)·m/S`
    的**整除差**推算 —— 两者是 floor 边界与 ceil 边界的差别，`S ≥ 2` 时**从不相等**
    （`T` 是 `S` 的倍数 ⇒ `S ∤ (T−1)`）。实测 `T=32,S=4` 真实笔数 `[8,8,8,7]`
    而旧口径给 `[7,8,8,8]`。
  - 后果三重：均值被放大/缩小；方差自由度同步错；`cnt` 被**低估**时
    `sq − sum²/cnt` 变负 ⇒ 方差夹到 0 ⇒ **IS Sharpe 被静默强制为 0**
    （低波动策略最易中招）⇒ argmax 改变 ⇒ φ 与 degradation 全错。
  - 改为 `count(b) = ceil((b+1)·m/S) − ceil(b·m/S)`，与累加**严格同源**。
  - 测试 `block_counts_match_brute_force_accumulation`（对照暴力计数）、
    `block_counts_sum_to_total_return_count`、
    `cscv_matches_materialized_reference`（对照物化参照实现，φ **逐位**一致）、
    `legacy_floor_based_counts_are_provably_wrong`（证明旧口径确为错）。
  - **注意：φ 数值会变** —— 旧值是错的。
- **`detail` / `fixed_curves` 不在缓存指纹里 ⇒ 第二次开 detail 拿不到任何产物**（High）
  - 缓存命中直接早退，走不到 detail 分支；而 detail 产物只由该分支写出。
  - 可复现死循环：先 `detail=false` 跑一次，再 `detail=true` 跑第二次 ⇒
    `detail/` 空 ⇒ `vta --pbo` 报"未找到 `*_fixedcurve.parquet`，请先用
    `detail=true` 重跑" —— 而用户**确实**设了 `detail=true`。
  - 产出集开关进指纹（`OutputSet` 值对象）。测试 `fingerprint_covers_output_set_switches`。
- **`read_dir` 的 IO 错误被 `filter_map(|e| e.ok())` 吞掉**（High）
  - `--pbo` 可能只读到一半曲线却返回"成功" ⇒ φ 基于**不同的策略集合**算出，无任何提示。
  - 改为逐项显式传播错误（`read_equity_curves` / `read_fixed_curves`）。
- **Parquet 的 NULL 被读成默认值**（High）
  - `Array::value(i)` 对 null 槽位返回类型默认值（`Float64`→`0.0`、`Utf8`→`""`、
    `UInt*`→`0`）。报告 schema 全部 `nullable = false`，故 null 只可能来自
    "别的工具写的文件"或"被手工改过的文件"。
  - 后果：`sharpe` 的 null → `0.0` → 与"真实零波动低风险"**不可区分**；
    `status` 的 null → `""` → 落到 `NoSignals` 分支，把"正常"误判成"退化"。
  - 改为**报错**（含列名与行号）。测试 `null_cells_are_rejected_instead_of_read_as_defaults`。
- **单位契约把两个比率/基点列声明成 `Money`**（Medium）
  - `expectancy` 是逐笔 `pnl_pct` 的**均值**（比率），`fixed_expectancy_bps` 是**基点**。
  - 原护栏只校验"每列都有声明"，不校验声明对不对 ⇒ 失守。
  - 新增 `semantic_units_match_the_engine_definition` 显式断言关键列的单位。
- **`stats.total_files - stats.failed_files` 可下溢**（Low）
  - `total_files` 是 `u32::try_from(len).unwrap_or(u32::MAX)`，组合文件数超过
    `u32::MAX` 时被截断，而 `failed_files` 仍按真实值累加 ⇒ debug 下 panic。改 `saturating_sub`。
- **`bbrk_vol` / `bbrk_vol_ema` 的文档与实现不符**（Medium）
  - 文档写 `mean(volume[i−N+1..i])`（不含当根），实现的前缀和覆盖
    `i−N+1..=i`（**含当根**）。以实现为准修正文档（与兄弟策略 `bbrk_rex` 同约定）。
- **`bbrk_pctb` 的 `%B` 守卫与另外三份抄本不同**（Medium）
  - 三份抄本用 `w <= 0.0`（拒绝负带宽），`bbrk_pctb` 内联用 `|up−lo| < 1e-12`
    （**接受负带宽** ⇒ `%b` 方向反转）。比值计算统一到 `shared::pct_b`，
    各自的入口守卫保持原样（行为不变）。
- **`bbrk_pctb_signals` 带一个从未使用的 `mid` 形参**（Low）
  - 靠 `let _ = mid;` 压住告警。死形参会让读者以为中线参与了决策。已删除。

### 资源安全

- **病态配置导致进程 abort**（Critical / High）
  - `ma_slope_atr_gate`（5 个维度）与 `swing_fractal`（范围型参数无长度上限）用
    **未封顶**的 `Vec::with_capacity(积)` 预分配 ⇒ 申请巨额内存失败 ⇒
    `handle_alloc_error` ⇒ **abort**（不可捕获，不是可读错误）。
  - 项目自己的 `shared::MAX_GRID_PREALLOC` 就是为此而设，且 40+ 个 config 都遵守，
  - 只有这 2 个漏了。根因是"封顶"逻辑被手写复制了 45 遍。
  - 现在**只有一处**实现：`shared::grid_prealloc(&[usize])`（`saturating_mul` + 封顶），
    43 个 config 全部改用它。测试 `grid_prealloc_is_capped_and_never_overflows`。
- **`IntParam::values()` 在 `step = 0` 时 panic**（Medium）
  - `step_by(0)` panic，而该函数是 `pub` 的（可绕过 `validate` 直接调用）。
  - 改为把 `step = 0` 视为"展开为空"，`validate` 仍报精确原因。
  - 测试 `values_does_not_panic_on_zero_step`。
- **写文件不是原子的**（Medium）
  - `File::create` 直接截断目标路径；写一半被杀 ⇒ 目标文件是**半截**的，
    而旧的可用版本已被销毁（而缓存有效性只看**行数**，挡不住"行数对、内容坏"）。
  - 改为先写 `{path}.tmp` 再 `rename`（POSIX 原子），失败时清理临时文件。
  - 测试 `failed_write_leaves_the_previous_file_intact`、
    `atomic_write_replaces_the_target_and_cleans_up`。

### 性能

- **`validate_columns` 在每个策略里全量重扫 4 条价格列**（High）
  - 它逐列全量扫描（O(T)），而 `run_combo` 按策略调用 ⇒ 同一份行情被扫 N 次
    （100+ 策略即 100 倍浪费）。与 `data_fingerprint` 同理**提到策略循环外**。
- **`params_to_json` 把 `split_underscore_pairs` 算两遍**（Low）。改为算一次。
- **`str_col` 每个字符串单元格分配 2 次**（Low，`to_string` + `clone`）。

### 架构与一致性

- **41 个 config 各自手写"预分配封顶"逻辑** → 收敛为 `shared::grid_prealloc`。
- **3 份逐字节相同的 `pct_b`** → 收敛为 `shared::pct_b`（并统一了第 4 份的口径）。
- **4 个 `ssf*` 策略的 `validate` 寄居在 `src/config.rs`**（距其类型 4000 行）
  → 下沉回各自的 `05_strategies/<name>/config.rs`，与另外 60+ 个 config 一致。
- **18 处硬编码的 BB 周期 `20`** → `shared::DEFAULT_BB_PERIOD`。
  （另有 9 个兄弟策略已把它暴露成 `bblen` 参数 —— 见「设计决策复核」第 3 条。）
- **`keltner` / `atrband` 的 EMA/SMA 周期与 ATR 长度裸写** → 具名常量
  `BASIS_PERIOD` / `ATR_PERIOD`（与各自文档口径一致）。
- **`bbrk_orth` 的空头离场分支漏了多头分支都有的有限性守卫** → 补齐。
- **公开 API 文档里的 U+FFFD 乱码**（`最长连��盈利笔数`）→ 修复。
- **引擎 `SummaryMetrics` 的 8 处重复文档注释**（重命名遗留）→ 合并。
- **`build.rs` 的 3 行文档注释重复了两遍** → 去重。
- **`swing_fractal` 的一处注释与代码相反**（说取 `[j]`，代码取 `[j-1]`）→ 修正。
- **19 个私有纯函数缺 `#[must_use]`** → 补齐。

### PBO 报告的可观测性

- `run_pbo_report` 原本三条跳过分支都是 `continue` + `warn!`，**全部组被跳过时**
  表里一行都没有、收尾仍打印判据 —— 用户以为"跑完了"，实际一个 φ 都没算。
  现在收尾打印"共 N 组参与计算 / M 组被跳过"。

## 2026-10 代码审查修复（rust-skills）· 第三轮（引擎时序与账目）

本轮聚焦**引擎内部**：成交时序、逐笔账目、统计矩公式。前两轮主要覆盖
策略层/IO 层，这三个缺陷此前一直藏在"测试全绿"之下。

### 正确性（静默出错类）

- **权益曲线在成交前按未来价格盯市**（Critical）
  - 旧 `scan_trades` 在**信号 bar** 就改 `cur_pos`/`cash`，成交价却取自
    `signal_bar + delay`。bar0 信号、bar1 开盘从 100 跳到 200 的用例中，
    bar0 权益被记成 `cash(0) + qty×close[0] = 500`（应为初始 1000），
    零收益策略算出 `sharpe = 11.22`、`mdd = 0`。
  - 改为 `VecDeque<Side>` 排队：信号 bar 只入队目标方向，到期 fill bar 才
    平仓/开仓/改现金。bar `i` 的权益只反映 bar `i` 及之前已发生的成交。
  - 影响：Sharpe、波动率、最大回撤、Sortino、Calmar、fixed 腿同名指标；
    **最终总收益不变**。回归测试
    `equity_curve_does_not_mark_to_future_fill_price`。
- **逐笔 `pnl` 漏扣开仓佣金**（Critical）
  - `CloseOutcome.pnl = qty×(exit−entry) − exit_fee`，而 `cash` 在开仓时已扣
    `entry_fee` ⇒ `sum(trade.pnl) != final_equity − initial_cash`。
    win/loss、profit factor、avg_win、SQN、偏度/峰度全部系统性偏乐观。
  - 现在 `pnl` 同时扣双边佣金；回归测试
    `trade_pnl_sums_to_equity_change_with_both_fees`（多/空/末尾强平三条路径）。
- **峰度公式第一项多乘 `n`**（High）
  - `m2`/`m4` 已是总体矩，校正系数应为 `(n²−1)/((n−2)(n−3))`，旧代码写成
    `(n+1)n(n−1)/((n−2)(n−3))`。
- **偏度比标准校正值小 `((n−1)/n)^1.5`**（Medium）
  - 旧代码把样本标准差 `√(m2/(n−1))` 当作总体 `m2^0.5`；n=10 时偏低约 14%。
  - 两条公式均改为总体矩 + Fisher-Pearson 小样本校正，并用
    `[1,2,3,4,10]` 对照 pandas `skew()=1.6970562748477143` /
    `kurt()=3.152` 锁定（`skewness_and_kurtosis_match_corrected_sample_moments`）。

> **报告可比性警告**：本轮修复前的报告，Sharpe/回撤/波动率/Sortino 与逐笔
> 统计（胜率、PF、SQN、偏度、峰度）均不可与新报告直接比较。旧缓存不会自动
> 失效的，请删掉 `temps/` 或加 `--no-cache` 重跑（源码哈希已变，正常情况下
> 指纹会自动失效）。

### 性能

- **缓存校验为"数行数"解码整张 Parquet**（High）
  - `run_combo` 原用 `read_parquet` 把全部列/全部行解码成 `Vec<RunResult>`，
    只为比较 `len() == param_count()`；每个策略、每次缓存命中都要付一次全表
    解码成本。
  - 新增 `output::parquet_row_count`，只读 footer 的 `num_rows`。完整
    schema/内容校验仍由 merge 阶段的 `read_parquet` 承担。

### 正确性 / 可观测性（次要）

- **PBO 同组重复 `params` / 非 `ok` / 不同时间网格的曲线未剔除**（Medium）
  - 重复跑或复制 `detail/` 会让同一参数的曲线出现多次，CSCV 把它当成多个
    "策略" ⇒ N 虚增、φ 被稀释。现按 `params` 去重并 `warn!` 去掉的条数。
  - 同时只取 `status == ok`；`no_signals` / `no_trades` / `bankrupt` 的收益率
    没有可比性，混入会稀释 φ。不同时间网格的曲线按精确 `time` 向量分组，
    选样本数最大的 cohort，不再按 `min(len)` 静默截断。
- **`validate_columns` 用 `lens[4].1` 取 close 长度**（Low）
  - 魔法下标与"按列名索引"的项目规则冲突；改为显式 `cols.close().len()`。
- **`build.rs` 每次成功构建都打 `cargo:warning`**（Low）
  - 源码哈希已通过 `VTA_SOURCE_HASH` 注入，成功路径无需再污染每一条 cargo
    命令的输出；失败/降级路径的 warning 保留。

### 复核后**未改**的项

- `aggregate_days` 首根 NaN 会让当日 `high/low` 保持 NaN：这是项目
  "NaN ⇒ 未知 ⇒ 策略 Hold"（P7）的既有契约，消费方（fiali/dualthrust/
  rbreaker）都显式 `is_finite` 检查。改成"取后续有限值"会把**未知**伪装成
  **已知的下界**，反而可能产生假信号。保留并在本节记录。
- `run_pbo_report` 用 `println!` 输出报告表：这是面向终端的正式产物，不是
  日志；`tracing::warn!` 只用于跳过/异常。保留。
- `build_one` 返回 `Option` 但全部分支都是 `Some`、`strategy_params_value`
  是手写多形态解析器：两者都值得后续收敛（前者可改直返 `Box`，后者可用
  `param_str` 的结构化编码替代），但改动面大、与本轮正确性修复无关，列为
  后续项。

## 2026-10 代码审查修复（rust-skills）· 第四轮（账目 / 策略契约 / 缓存 / PBO）

按“引擎账目 → 策略契约 → 缓存与 PBO → schema 与文档”四批落地，每批都过了
`fmt → check → clippy -D warnings → test → 冒烟`。

### 引擎账目

- 现金 ≤ 0 时禁止**新开仓**（只允许平掉已有仓位），并新增 `RunStatus::bankrupt`；
  旧引擎会以负现金继续计算持仓量，数量变负即方向反转，报告出现无法解释的权益。
- `open` 短于 `close` 时，缺失的开盘价回退到收盘价；不再把 `open.len()` 算进
  `n` 的 `min`，避免 `open = []` 时整段信号被静默丢弃。
- `final_equity` 直接取账本精确值，不再由 f64 曲线末点回造。
- 所有返回路径（包括提前返回）都回填执行口径，报告不再出现“数据不足行是 0、
  正常行是真实值”的混排。
- 保本交易（pnl = 0）不再计入亏损：胜率分母为“胜 + 负”，零盈亏不打断连赢/连亏，
  runs 检验跳过。

### 策略契约

- 12 个仍有“算不出来就全 Hold”残余降级的策略改为返回 `ParamSkipped`：
  `macd`、`vwap`、`ma_atr`、`ma_slope_atr_gate`、`mabb_long`、`mabb_short`、
  `bbrk_ttm_fire`、`bbrk_hma_v{1..5}_sl`。`invalid` 与 `no_signals` 不再混为一谈。

### 缓存与 PBO

- `output.fixed_curves = true` 必须同时 `detail = true`，否则配置期报错。
- detail 产物增加组合侧车清单、全局 `detail/manifest.toml` 索引、`combo`/`status`
  metadata；fixedcurve 改 `time` 列。缓存命中要校验整组产物，`--pbo` 只读索引。
- PBO 只取 `status == ok`，按精确时间网格选最大 cohort，不再 `min(len)` 截断；
  状态/窗口/重复参数的丢弃量都 `warn!`。

### schema 与文档

- 删除与 `recovery_factor` 完全重复的 `return_mdd` 列；`calmar` 保留（CAGR/MDD）。
- detail trades 的金额列 `total_return` → `pnl`，与报告的比率列 `total_return`
  彻底分开。
- `ExecConfig::try_from_bps_fill` 拒绝负/非有限费率与滑点；配置路径之外的公开
  引擎 API 也不能绕过这条约束。
- 修正“`simulate` 内存 O(1)”的过度声明：不物化曲线，但逐笔 pnl 序列仍需
  O(交易笔数) 用于高阶矩与分半 PF。

**兼容性**：`result.parquet` 少一列、detail trades 改列名、fixedcurve 改列名，
历史文件不兼容；源码哈希已变，正常缓存会自动失效。detail 目录建议删除后重跑，
让侧车清单与索引重建。

## 设计决策复核（已确认并实施）

以下几项**不是 bug**，而是设计意图本身有疑点。经确认后已按下列方案实施，
每项都列出了"改了什么"与"代价是什么"。

### D1. 66 个策略在算不出时**静默降级**为全程 Hold（`Err(_) => 全 Hold`，95 处）

`Strategy` trait 的文档明写"返回 `Err` 表示该参数/数据无法计算，**不做静默降级**"，
但 66/67 个策略做的恰好相反：指标算失败时返回**全 Hold** 的成功结果。
只有 `vwma` 守住了契约。

后果：某个参数组合算不出来时，引擎把它当成"正常运行但没信号" —— 报告里
`trades=0 / pnl=0 / sharpe=0`，与"零波动低风险"的真实结论**数值上不可区分**。
对**参数网格过拟合研究**（本项目的 PBO 模块）而言，静默空网格正是最该被发现的
失败模式。

**实施（方案 B）**：新增 `ParamOutcome`（`Signals` / `Skipped`）与
`AppError::ParamSkipped`；`generate_signals_batch` 改为**逐档**返回结果。
算不出的档位 → `status = "invalid"`，**不中断**整轮回测，报告里显式可见。
95 处静默降级点全部改为显式 `Err(ParamSkipped)`（含可读原因，如
"ema: Invalid period: period = 1420, data length = 1004"）。

**代价**：报告 `status` 列新增取值 `invalid`；旧缓存文件读回时落到未知状态分支
（保守落 `NoSignals`，不臆断）。`RunStatus::Invalid` 必须与 `NoSignals` 配对解读。

### D2. 缓存指纹不含**代码/实现版本**

指纹只覆盖"输入"（数据/资金/窗口/口径/参数/产出开关），不含"**实现**" ——
改完策略代码重跑会**全部命中旧缓存**，报告里是修复前的结果，且**无任何提示**。

**实施（方案 ①+③）**：
- **①** 新增 `build.rs`：对本 crate 的 `src/` 与两个 path 依赖
  （`vector-ta` / `kline-reader`）的源码做**内容哈希**（FNV-1a），经
  `cargo:rustc-env=VTA_SOURCE_HASH` 注入，指纹混入它。
  用**内容哈希**而非 `git rev-parse` 的理由：monorepo 里改一个无关的兄弟 crate
  不该让缓存全量失效；非 git 检出时 `git` 不可用；**工作区有未提交改动时
  commit hash 不变** —— 恰恰是"刚改完代码"这个最需要失效的时刻不会失效。
  找不到源文件时降级为哨兵 `unknown` 并 `cargo:warning`，不阻断构建。
- **③** 新增 `--no-cache` 逃生门：忽略全部缓存强制重算。

**代价**：改任何一行源码（含注释）都会让缓存失效 —— 这是有意的。

### D3. 9 个兄弟策略把 BB 周期暴露成 `bblen` 参数，16 处硬编码 `20`

**实施**：16 个策略全部加上 `bblen: PeriodParam`（**缺省 20 ＝ 旧串语义**），
成为独立的网格维度；裸写的 `20` 统一为 `shared::DEFAULT_BB_PERIOD`。
`params` 列随之带上 `bblen=` 键（如 `sigma=2.5,bblen=20`）。

**代价**：默认配置（`bblen` 缺省 20）的**网格规模与数值完全不变**（已逐列比对
验证）；但 `params` 列文本与 `strategy_params` 编码**变了**，历史报告的
`params` 不可直接与新报告对齐。主动设置 `bblen` 范围会增大 N ⇒ PBO 噪声底线上移。

### D4. 7 个策略用**盘中**止损判定，与框架自身的成交模型冲突

`swing_fractal` 的文档明写"止损为**收盘价**判定，非盘中 stop 单"（成交价只由
`ExecConfig` 决定，无法表达"盘中触及即按止损价成交"），但 `mabb_long/short`、
`bbrk_kdj`、`bbrk_hma_v1/v2/v3/v4/v5_sl` 用的正是 `low[i] <= stop` 形态 ——
止损在 bar `i` **盘中**判定触及，却按 bar `i+1` 的开盘成交，实际盈亏由**跳空**
决定（系统性高估），与框架的延迟口径冲突。

**实施**：11 处盘中判定全部统一为**收盘价**判定，与 `swing_fractal` 同口径，
并在每个函数上写明理由。

**代价**：这 7 个策略的**历史结果失效**（数值改变）。这是"统一"的必然代价。

**后续（引擎级 stop）**：新增的 `[[strategy]].stop` 已能在引擎层表达“盘中触及
即按止损价成交、跳空按开盘价成交”，因此 D4 中“框架无法表达盘中止损”的前提
不再成立。但**策略内建止损**（包括 `_sl` 变体）仍保留 D4 的收盘价口径，属于
遗留实现；新策略应使用引擎 `stop`，不要继续复制 `_sl`。引擎 stop 与旧 `_sl`
的语义差异见「持仓期止损（引擎级 `stop`）」一节。

### D5. 30+ 个 bar 数参数用 `f64` 承载再 `.round() as usize`

**实施**：全部换成既有的 `PeriodParam(usize)`（新增 `values()` / `validate_range()`
两个整型版本，展开上限 `MAX_PERIOD_RANGE` 防爆）。`bblen = 20.4` 这类值现在
在**反序列化期**即被拒（原先静默取整成 20，两个不同 TOML 值映射到同一参数串）。

**代价**：配置 schema 变更。现有配置里的这些字段**本来就是整数**，故全部照常
可用（已核对 `config.*.toml`）。

### D6. 报告文件名由 `symbol` / `timeframe` / `params` 拼出 ⇒ 路径穿越与文件名碰撞

**实施**：
- `symbol` / `timeframe` 走白名单校验（`[A-Za-z0-9_.-]+`，禁路径分隔符与
  `..`）—— 它们**直接进入落盘文件名**。
- detail 文件名改为 `可读片段 + 参数串 FNV-1a 哈希`（`{h:08x}`，8–16 位十六进制）：既保留可读性，
  又保证不同参数串**必定**不同文件名（旧的清洗规则 `a=b` 与 `a-b` 会撞名，
  导致**不同参数组互相覆盖**）。

**代价**：detail 文件名多了哈希后缀（可读性略降，`params` 列不受影响）。

### D7. 资金以裸 `f64` 流通，`InitialBalance` 新类型在第一次使用处即被拆掉

**实施**：引入 `src/money.rs` 的 `Amount`（`rust_decimal::Decimal` 定点十进制），
把**钱**从 `f64` 里彻底搬出来：
- `InitialBalance` → `Amount`（不再是"给 f64 套个名字"）；
- 引擎钱账本（`cash` / `pos_qty` / `fixed_qty` / `fixed_realized` / 逐笔
  `pnl` / `cash_delta` / `fixed_pnl`）全部 `Amount`，算术**全部返回 `Result`**
  （溢出/除零显式报错，不静默回绕或产出 `Inf`）；
- `SummaryMetrics` 的货币字段（`final_equity` / `avg_win` / `avg_loss` /
  `total_fixed_pnl`）与 `RunResult.buy_hold_pnl` → `Amount`；
- Parquet 报告的 6 个货币列与 detail trades 的 3 个货币列 → **`Decimal128(38, 8)`**。

**边界（刻意保留 `f64`）**：价格（要参与 `ln`/`sqrt`）、比率（无量纲）、
权益曲线**序列**（供 `CurveAccum` 与 PBO 的 CSCV 用，需要超越函数）。
这些在 `src/money.rs` 的模块文档里逐条写明。

**代价**：报告与 detail 的**历史文件不兼容**（金额列类型变了）；`simulate` /
`simulate_detailed` / `buy_hold_pnl` 等公开 API 的签名变了
（`initial_cash: f64` → `Amount`，返回值改为 `Result`）。

## 完整架构

```
main.rs             # 入口：cli::parse → -c 回测 / --pbo 实算，二选一；anyhow 收敛
lib.rs              # 只做导出：#[path] 挂载 app/NN_*（config/data/engine/output/strategies）+ pub mod app/cli/error
cli.rs              # clap 4 derive，--config/-c 与 --pbo 互斥；--no-cache 逃生门
money.rs            # Amount(rust_decimal 定点十进制): 钱的唯一类型
build.rs            # 构建期源码内容哈希 → VTA_SOURCE_HASH(进缓存指纹)
config.rs           # Config{data/backtest/output/strategy} + DataSection::for_pair 降单值
error.rs            # thiserror AppError(Io/Config/Data/Parquet/Arrow/Msg)
pbo.rs              # CSCV 实算 + 单测
app/mod.rs          # run/run_combo：validate → build → 双层串行取数 → 策略层 → temps/detail → merge result.parquet → detail 索引
app/03_run_backtest.rs # engine：simulate/simulate_detailed + simulate_ohlc/simulate_detailed_ohlc + CurveAccum + SummaryMetrics
app/04_write_output/parquet.rs # temps/detail 写 + ColumnIndex(按列名读) + 单位契约 COLUMNS + merge
app/05_strategies/  # Strategy trait + shared + build + ~80 策略叶
```

## 完整配置

```toml
[data]
type = "binance" # 必填，"binance" | "dukascopy"
market = "futures/um" # 可选，Binance 市场："spot" | "futures/um" | "futures/cm"，默认 futures/um
dir = "/data/nutilustrader/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = "BTCUSDT" # 必填，单值或数组
timeframe = "1d" # 必填，单值或数组
# start_date = "2024-01-01" # 可选
# end_date = "2024-12-31" # 可选
[backtest]
initial_balance = 10000.0 # 必填，>0
# delay_bars: 可选, 默认 1, **必须 >= 1**(0 是前视乐观口径, 反序列化期即被拒)
# fill_bar_price: 可选, "open"(默认) | "close"
# fee_bps / slippage_bps: 可选, 默认 0, 非负且有限
[output]
dir = "/data/nutilustrader/outputs/reports/vta" # 可选，默认 "reports"
detail = false # 可选，默认 false
fixed_curves = false # 可选，默认 false；仅当 detail = true 时允许
overwrite = false # 可选，默认 false
[[strategy]] # 必填，至少一个
type = "sma"
period = { start = 5, end = 100, step = 5 } # 范围或单值
# 可选：该条策略专属的持仓期止损（百分比，缺省关闭）。
# 同一策略可重复写多条 [[strategy]]，每条配不同 stop，即形成止损网格。
# stop = { stop_loss_pct = 2.0, trailing_stop_pct = 5.0 }
# 也可写成表：先写 [[strategy]]，再写 [strategy.stop] 与两个键。
```

- 未知键拒收；读写/解析失败带文件路径。
- 全量示例见 `config.example.toml`；`config.ma_all_1h.toml` / `config.sma_100.toml` 为常用组合；`scripts/` 下有定稿配置。

### 持仓期止损（引擎级 `stop`）

`stop` 是**每条 `[[strategy]]` 独立**的运行参数，不是全局开关，也不属于策略代码。
它只作用于引擎已成交的持仓，策略信号数组本身不变；但止损平仓后引擎会把目标
方向重置为 `Flat`（见下），因此后续 `Hold` 不会自动重入。

- 固定止损：多头 `entry × (1 - stop_loss_pct/100)`，空头
  `entry × (1 + stop_loss_pct/100)`。
- 追踪止损：多头 `extreme × (1 - trailing_stop_pct/100)`，空头
  `extreme × (1 + trailing_stop_pct/100)`；`extreme` 是入场后的最高 high /
  最低 low，**在每根 bar 收盘后更新，供下一根 bar 判定**。
- 两者同时配置时取更紧的一侧（多头取较大值，空头取较小值），平仓原因记为
  `stop_loss` 或 `trailing_stop`。
- 两个字段都必须严格位于 `(0, 100)` 内；0、100、负数与 NaN 在配置校验期即被拒绝。
- 盘中用 `high/low` 判定触发：多头 `low <= stop`、空头 `high >= stop`。
  正常按止损价成交；若开盘已跳空穿过止损（多头 `open <= stop`、空头
  `open >= stop`），按 `open` 成交。之后仍按 `ExecConfig` 扣逆向滑点与双边佣金。
- `fill_bar_price = "open"`（默认）：先执行到期的开盘成交，再在同一根 bar
  检查止损，因此开盘新仓当根就可能被止损。
- `fill_bar_price = "close"`：先检查盘中止损，再执行到期的收盘成交，避免用
  收盘价“跳过”盘中已经发生的止损。
- 收盘价入场的那根 bar 不回溯使用该 bar 的 high/low；开盘价入场则可以当根触发。
- 止损平仓后目标方向重置为 `Flat`，并作废止损前已经排队的目标；后续 `Hold`
  不会自动重入，只有新的 `Buy` / `Sell` 信号才会在 `delay_bars` 后重新入场。
- 未配置 `stop` 时，引擎完全不读 high/low，结果与旧路径逐位一致；`delay_bars >= 1`
  的口径也不变。

同一策略配多个止损档位时，重复 `[[strategy]]` 即可。报告 `params` 会追加
`;stop_loss_pct=...` / `;trailing_stop_pct=...` 后缀，缓存指纹包含 stop，
报告新增 `stop_loss_exits` / `trailing_stop_exits` 两列（整数计数）。

> **遗留 `_sl` 策略**：`bbrk_hma_v1_sl` … `bbrk_hma_v5_sl` 仍是旧实现 ——
> 止损只作用于 `pos=2` 的逆势腿，距离用入场根冻结的 `stopk × ATR`，且按收盘价
> 判定，与这里的“全持仓、百分比、盘中 high/low”口径**不同**，历史结果不可直接
> 对比。它们暂时保留用于回归；引擎 stop 验证通过后再删除。

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
  - `run`：预算目录 → 双层串行 → `load_columns`（失败跳过）→ `run_combo`（缓存/并行/写盘）→ `merge` → `result.parquet` → `detail/manifest.toml`
- app/03_run_backtest.rs
  - `simulate`/`simulate_detailed` 与带 high/low 的 `simulate_ohlc`/`simulate_detailed_ohlc`：信号→持仓→成交→30 项绩效
- app/04_write_output/parquet.rs
  - `temps/` 单组合写 + `detail/` 明细写 + `read` 校验 + `merge`
- app/05_strategies/
  - `build.rs` 工厂按 `[[strategy]]` 展开参数实例，并把该条的 `stop` 一并带入作业
- pbo.rs
  - 读 `detail/manifest.toml` 索引 → 筛 `status == ok` / 同时间网格 → 分组 CSCV → 报告

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

另有两个**审查用冒烟配置**（规模小、覆盖多条关键路径，供回归核对）：

| 文件 | 覆盖点 |
|---|---|
| `config.review_smoke.toml` | 多品种 × 多周期 × 多策略 + `detail` + `fixed_curves` + `--pbo` 全链路 |
| `config.review_wide.toml` | 本轮审查触及的 16 个策略（`pct_b` 家族 / BB 周期常量化 / `vwap` 长度口径 / 网格封顶 / `ssf` 校验下沉 …） |
| `config.review_invalid.toml` | 验证「参数不可计算」在报告里显式可见（`status = "invalid"`） |
| `config.review_bblen.toml` | 验证 `bblen` 成为独立网格维度（σ × BB 周期） |

两者都写死了 `/data/nutilustrader/data` 与 `/data/nutilustrader/outputs/...`
路径，换机器需改 `data.dir` 与 `output.dir`。
