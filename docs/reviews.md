# 代码审查记录（2026-10）

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
  - 现在是单个**穷尽 match**（`build_one`，无 `_` 兜底），编译器强制覆盖全部 82 个变体。
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
- **4 个 `ssf*` 策略的 `validate` 寄居在中央配置模块**（距其类型 4000 行）
  → 下沉回各自的 `src/strategies/<name>/config.rs`，与另外 60+ 个 config 一致。
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

- 7 个仍有“算不出来就全 Hold”残余降级的策略改为返回 `ParamSkipped`：
  `macd`、`vwap`、`ma_atr`、`ma_slope_atr_gate`、`mabb_long`、`mabb_short`、
  `bbrk_ttm_fire`。`invalid` 与 `no_signals` 不再混为一谈。

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
- **①** 新增 `build.rs`：对本 crate 的 `src/`、两个 reader 的 `src/`
  （`binance-data-reader` / `dukascopy-data-reader`）与 `Cargo.lock`（锁定 `vector-ta` 版本）做**内容哈希**（FNV-1a），经
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

### D4. 策略内建止损曾用**盘中**判定，与框架自身的成交模型冲突

`swing_fractal` 的文档明写"止损为**收盘价**判定，非盘中 stop 单"（成交价只由
`ExecConfig` 决定，无法表达"盘中触及即按止损价成交"），但 `mabb_long/short`、
`bbrk_kdj` 以及当时仍存在的 `bbrk_hma_v1/v2/v3/v4/v5_sl` 用的正是
`low[i] <= stop` 形态 —— 止损在 bar `i` **盘中**判定触及，却按 bar `i+1` 的
开盘成交，实际盈亏由**跳空**决定（系统性高估），与框架的延迟口径冲突。

**实施**：当时这些策略中的盘中判定统一为**收盘价**判定，与 `swing_fractal`
同口径，并在每个函数上写明理由。

**代价**：当时受影响策略的**历史结果失效**（数值改变）。这是"统一"的必然代价。

**后续（引擎级 stop）**：新增的 `[[strategy]].stop` 已能在引擎层表达“盘中触及
即按止损价成交、跳空按开盘价成交”，因此 D4 中“框架无法表达盘中止损”的前提
不再成立。五个 `_sl` 变体已由引擎 stop 取代并删除；仍保留的 `mabb_long/short`、
`bbrk_kdj` 按 D4 的收盘价口径执行策略内建止损。新策略应使用引擎 `stop`，
不要继续复制 `_sl`。引擎 stop 与旧 `_sl` 的语义差异见
「持仓期止损（引擎级 `stop`）」一节。

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

