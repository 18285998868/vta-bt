# 只读分析层

`vta-analysis` 读取 `vta` 已产出的报告目录, 输出对账、描述性统计与单组合显著性结论。它不重跑回测, 不修改 `result.parquet` 或 `detail/` 下的任何文件。

## 目标与边界

做:

- 校验 `result.parquet` 的引擎指标与 `detail/` 曲线/逐笔是否一致;
- 按 fixed 腿(固定名义、不复利)排序, 复利腿并列展示;
- 给出 PSR、DSR、MinTRL、Lo 标准误、平稳 bootstrap 置信区间与组内 BH q 值;
- 输出回撤区间、月/年收益、交易结构、参数边际、PBO 摘要与排除清单。

不做:

- 不重跑回测, 不做 walk-forward / CPCV 编排;
- 不生成图表, 绘图交给下游脚本读 CSV/JSON;
- 不把参数平原或 PBO 当成样本外证据;
- 不跨品种、周期、策略、方向、费用、执行口径或数据窗口混排。

## 输入契约

```text
<report>/
  result.parquet                 # 必需; 引擎指标是权威值
  detail/manifest.toml           # 可选; 有则进入曲线级分析
  detail/*_fixedcurve.parquet    # 可选
  detail/*_equitycurve.parquet   # 可选
  detail/*.parquet               # 可选; 逐笔成交
  temps/<combo>.detail.toml      # 可选; 产物大小侧车清单
```

`detail/manifest.toml` 的 `combo` 是一轮运行的缓存 id, 同一 combo 下可以有多条参数曲线。因此曲线身份必须用 `(combo, params)` 或 `(symbol, timeframe, strategy, params)` 定位, 不能只用 combo。

`detail/` 缺失时仍可做摘要分析; 加 `--require-detail` 可把缺失变成硬错误。

## 命令

```bash
vta --analyze <report> [--out <dir>] [--top 20] [--bootstrap 2000]
```

| 参数 | 默认 | 说明 |
|---|---|---|
| `--analyze <DIR>` | 必填 | 报告目录; 与 `-c` / `--pbo` 三选一 |
| `--out <DIR>` | `<report>/analysis` | 产物目录 |
| `--top <N>` | 20 | Markdown 行数与表格覆盖的组合数 |
| `--bootstrap <N>` | 2000 | 平稳 bootstrap 次数; 0 跳过 |
| `--block-len <BARS>` | 自动 | 固定块长; 省略时用 Politis-White |
| `--seed <U64>` | 42 | 随机种子 |
| `--alpha <P>` | 0.05 | MinTRL 与置信区间 |
| `--min-trades <N>` | 20 | 参与排名/显著性的最少成交笔数 |
| `--trials <N>` | 自动 | DSR 试验数; 必须 >= 1 |
| `--max-curves <N>` | 2000 | detail fixed 曲线数上限, 防 OOM |
| `--require-detail` | 关 | 缺 `detail/manifest.toml` 直接报错 |

不指定 `--out` 时会在报告目录下新建 `analysis/`; 只新增文件, 不触碰原有产物。

## 分组与口径

分组键:

```text
(symbol, timeframe, strategy, direction,
 fee_bps, slippage_bps, delay_bars, fill_close,
 window: (bars_used, buy_hold_return, buy_hold_pnl))
```

同一组内再按精确时间网格划分 PBO cohort。不同费用、方向、窗口或时间网格的组合不参与同一排序。

硬排除, 任一命中即不参与排名:

- `status != ok`;
- 周期不在年化表内;
- 成交笔数低于 `--min-trades`;
- detail 连接失败(同一四元组有多条不同曲线);
- 对账失败。

缺 detail 但不违反上述条件的行可以参与排名, 只是不能做曲线级显著性; 报告会单独计数。

排名按 fixed 夏普降序, 依次比较 fixed 总收益与组合 id。fixed 腿为固定名义、不复利, 适合跨组合比较; 复利腿仅并列展示。

## 对账

对每个能连上 detail 的组合, 用曲线复算并对照引擎列:

| 检查 | 容差 |
|---|---|
| `bars_used` 与 fixed 曲线长度 | 0 |
| fixed 夏普 / 最大回撤 / 总收益 | 1e-8 绝对差 |
| 复利夏普 / 最大回撤 / 总收益 | 1e-8 绝对差 |
| 成交笔数、止损/追踪/策略止损离场数 | 0 |
| fixed 盈亏合计 | 0.01 |
| `final_equity` 与首点权益 + 逐笔 pnl 合计 | 0.01 |
| 胜率、profit factor、平均持仓 | 1e-8 |

任一检查失败会写入 `exclusions.csv` 的 `reconciliation_failed` 并排除该组合。报告不会用曲线值覆盖引擎列。

## 输出

```text
<out>/analysis.json       # 权威结构化结果, schema_version = 2
<out>/analysis.md         # 人读报告
<out>/tables/combos.csv
<out>/tables/groups.csv
<out>/tables/significance.csv
<out>/tables/drawdowns.csv
<out>/tables/calendar.csv
<out>/tables/trades.csv
<out>/tables/param_marginals.csv
<out>/tables/exclusions.csv
<out>/tables/status.csv
```

`analysis.json` 的顶层字段:

| 字段 | 内容 |
|---|---|
| `schema_version` / `tool_version` / `source_hash` | 版本与源码哈希 |
| `input_digest` | 输入指纹 |
| `options` | 本次分析参数 |
| `input` | 行数、detail 文件数、品种/周期/策略、时间范围 |
| `coverage` | 状态、可排名、可显著性、缺 detail、对账不一致计数 |
| `groups` | 分组统计、参数稳定性、PBO |
| `combos` | 全部组合(可排名在前) |
| `top_ids` | Top-N 组合 id |
| `detail_top_ids` | Top-N 中有 detail 的组合 id |
| `drawdowns` / `calendar` | 有 detail 的 Top-N 的回撤与月/年收益 |
| `trade_stats` | 所有有 detail 的组合的交易结构 |
| `parameter_marginals` | 各参数取值的边际统计 |
| `exclusions` / `warnings` | 硬排除与非致命警告 |

`input_digest` 是 FNV-1a 指纹, 覆盖 `result.parquet` 字节、manifest 字节、每行身份与 fixed 曲线的 `(combo, params, 长度, 首尾时间)`。它不哈希整条曲线, 避免为了追溯再读一遍 GB 级文件; 曲线完整性由 `read_detail_bundle` 的文件大小/combo/params/时间轴校验保证。

## 统计方法

以下 SR 均为逐 bar 夏普, 年化值乘 `sqrt(bars_per_year)`。`T` 是收益笔数, 即曲线长度减一。

### PSR

```text
PSR(SR*) = Φ( (SR - SR*)·sqrt(T-1)
              / sqrt(1 - γ3·SR + ((γ4-1)/4)·SR²) )
```

`γ3` 为总体偏度, `γ4` 为原始峰度(正态 = 3)。分母非正时返回不可计算, 不夹到 0/1。`psr_p_value = 1 - PSR(0)`。

### Lo 标准误

```text
SE(SR) = sqrt( [1 + 0.5·SR² - γ3·SR + ((γ4-3)/4)·SR²] / T )
```

这是 iid 假设下的解析近似; 有自相关时应看平稳 bootstrap 区间。报告中的 `lo_se_annual` 已乘 `sqrt(bars_per_year)`。

### DSR

```text
SR*_0 = sqrt(V[SR_n]) · [ (1-γ)·Φ⁻¹(1 - 1/N)
                         + γ·Φ⁻¹(1 - 1/(N·e)) ]

DSR = PSR(SR*_0)
```

`γ` 为 Euler-Mascheroni 常数, `N` 是试验数。默认 `N` 取同组同窗口 `status == ok` 的行数, `V[SR_n]` 也在这批行上计算; 失败或样本不足的试验不会被从选择偏差里抹掉。`--trials` 可覆盖 `N`, 但方差仍来自实际行。`N <= 1` 时 DSR 退化为 PSR(0)。

参数高度相关时, 这个 `N` 偏保守; 报告同时给出 `trial_sr_variance` 与 `expected_max_sr_annual`, 便于复核。

### MinTRL

```text
MinTRL = 1 + [1 - γ3·SR + ((γ4-1)/4)·SR²]
             · (Φ⁻¹(1-α) / (SR - SR*))²
```

返回收益笔数; 报告中的 `min_trl_bars` 已加 1 换算成 bar 数, `min_trl_ok` 表示实际长度是否达到。`SR <= SR*` 时不可达, 输出空值。

### 平稳 bootstrap

按 Politis-Romano 平稳 bootstrap 重采样逐 bar 对数收益, 块长由 Politis-White 自动选择, 也可用 `--block-len` 固定; 输出年化夏普的百分位置信区间。每个组合的随机种子为 `seed XOR FNV1a(combo_id)`, 因此增删其它组合不会改变本组合的区间。报告同时给出 `block_len`、`resamples`、`mean_sr`、`se_sr`。

### BH q 值

只在同组内对 `psr_p_value` 做 Benjamini-Hochberg。跨组混排没有 FDR 意义。若同组只有部分行有 p 值, 报告会警告 `BH 仅覆盖 m/n 行; q 值偏乐观`。

### PBO

复用 `vta-pbo` 的 CSCV 实现, 只使用可排名、有 detail 且共享同一精确时间网格的 fixed 曲线。曲线数少于 10 或 `T < 32` 时不计算并给出警告。PBO 是组内统计, 不跨组汇总。

### 回撤与尾部

- 回撤 `dd_t = 1 - equity_t / peak_t`, 从创新高后的第一个下跌 bar 到恢复或样本结束;
- 权益归零或转负时深度按 100% 记录, 不再按比例外推;
- Ulcer Index 为 `sqrt(mean(dd_t²))`;
- CVaR 取最差 5% 逐 bar 对数收益的均值;
- 回撤区间按深度降序, 每个组合最多 10 段。

### 日历收益

按 UTC 月/年聚合。首个周期的基准是样本首点权益, 之后每个周期的基准是上一周期末权益; `return_ratio` 是小数, `max_drawdown` 也以周期起点权益为基准。

### 交易结构

统计方向、胜负、盈亏分布、profit factor、持仓期、MAE/MFE 与离场原因。`MFE/MAE` 列为 `avg_mfe / avg_mae`, `MAE = 0` 时为空。

### 参数稳定性

按 `strategy_params_json` 的键值对拆参数。边际统计给出每个取值的行数、均值/中位数/标准差与平均收益; 邻域定义为其它参数完全相同、该参数取相邻数值。最优夏普大于 0.5 且比最好邻居高出 0.5 以上时标记孤立尖峰。这些都是描述性统计。

## 复现

- 固定 `seed`、固定排序、原子写文件;
- 同一输入与参数重复运行, `analysis.json`、`analysis.md` 与全部 CSV 逐字节一致;
- 不依赖调用者 cwd, 所有路径由 `--analyze` / `--out` 决定。

验证命令:

```bash
cargo test -p vta-analysis
cargo clippy -p vta-analysis --all-targets -- -D warnings
cargo run -p vta --release -- --analyze reports/vta_review_smoke --out /tmp/analysis-smoke
```

## 假设与限制

- PSR/DSR/MinTRL 假设收益平稳; 非平稳序列只能用 bootstrap 区间做辅助判断;
- DSR 的试验数不是独立试验数, 参数相关时偏保守;
- BH 控制的是组内 FDR, 不是全报告 FWER;
- 缺 detail 的行无法做曲线级检验, 只能参与摘要排名;
- PBO 需要同组至少 10 条同网格曲线, 小网格会直接跳过;
- 对账只覆盖已实现的检查项, 未覆盖的指标仍以引擎列为准。

## 参考文献

- Bailey, D. H. & López de Prado, M. (2012). The Sharpe Ratio Efficient Frontier. Journal of Risk 15(2).
- Bailey, D. H. & López de Prado, M. (2014). The Deflated Sharpe Ratio. Journal of Portfolio Management 40(5).
- Bailey, D. H., Borwein, J., López de Prado, M. & Zhu, Q. J. (2016). The Probability of Backtest Overfitting. Journal of Computational Finance 20(4).
- Lo, A. W. (2002). The Statistics of Sharpe Ratios. Financial Analysts Journal 58(4).
- Politis, D. N. & Romano, J. P. (1994). The Stationary Bootstrap. JASA 89(428).
- Politis, D. N. & White, H. (2004). Automatic Block-Length Selection for the Dependent Bootstrap. Econometric Reviews 23(1).
- Benjamini, Y. & Hochberg, Y. (1995). Controlling the False Discovery Rate. JRSS-B 57(1).
- Martin, P. & McCann, B. (1989). The Investor's Guide to Fidelity Funds.
- Rockafellar, R. T. & Uryasev, S. (2000). Optimization of Conditional Value-at-Risk. Journal of Risk 2(3).

相关文档: [架构与执行流程](./architecture.md), [CSCV / PBO](./pbo.md), [关键设计约束](./invariants.md)。
