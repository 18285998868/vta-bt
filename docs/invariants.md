# 关键设计约束（改动前必读，18 条）

这些不是风格偏好，而是踩过坑之后定下来的硬约束。违反它们会**静默产出错误结果**。

### 1. 外层三层循环必须串行（品种 × 周期 × 策略）

`app/run.rs::run_with` 的三个外层循环**刻意保持串行**，不得"优化"为并行：

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
| `Bars` / `Count` | bar 数 / 整数计数 | `avg_hold_bars` / `trades`、`stop_loss_exits`、`trailing_stop_exits`、`strategy_stop_exits`、`bars_used`、`delay_bars`、`consecutive_*` |
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

