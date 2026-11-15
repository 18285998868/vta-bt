# C6 只读报告分析层

## 目标

1. 在现有回测产物之上提供独立的只读分析入口, 不重跑回测。
2. 引擎指标保持权威, 曲线复算只用于对账; 不一致必须显式排除。
3. 显著性结论标注假设、试验数与样本长度, 并复用既有 CSCV/PBO 实现。
4. 输出确定性: 固定 seed、固定排序, 同一输入重复运行逐字节一致。

## 范围

本步只交付阶段 1+2:

- 输入装载、detail 完整性校验、对账;
- 描述性统计、回撤、交易结构、日历、参数边际、PBO 摘要;
- Lo 标准误、PSR、DSR、MinTRL、平稳 bootstrap、组内 BH。

阶段 3(White Reality Check、Hansen SPA、Romano-Wolf、有效试验数)不在本步。

## 设计

1. 新增独立 crate `vta-analysis`, 依赖 `vta-core`、`vta-report`、`vta-pbo`; 不反向依赖策略实现。
2. `bars_per_year` / `is_known_timeframe` 下沉到 `vta-core::timeframe`, 引擎、分析层与 PBO 共用同一张年化表。
3. `vta-report` 增加 `read_detail_bundle`, 一次读取 manifest、fixed/equity/trades 并校验文件大小、combo、params、status 与时间轴。
4. 分组键包含品种、周期、策略、方向、费用、滑点、延迟、成交价口径与精确数据窗口; 不同口径不混排。
5. 默认用 fixed 腿做跨组合比较; 复利腿并列展示。
6. 曲线级表格只覆盖有 detail 的 Top-N; 名次仍按全量排名标注, 缺 detail 的组合只跳过曲线级表格。
7. DSR 的试验数取同组同窗口 `status == ok` 行数, 不用可排名行数; 避免把失败试验从选择偏差里抹掉。
8. 输出 `analysis.json`(schema 2)、`analysis.md` 与 `tables/*.csv`, 不做图表。

## 关键口径

- PSR/DSR/MinTRL 使用逐 bar 夏普与总体矩; 峰度为原始峰度。
- MinTRL 内部返回收益笔数, 对外加 1 换算成 bar 数。
- BH 只在同组内计算; 覆盖不足时写警告。
- PBO 只使用可排名、有 detail、共享同一时间网格的 fixed 曲线, 至少 10 条且 `T >= 32`。
- 平稳 bootstrap 每个组合用 `seed XOR FNV1a(combo_id)`, 增删其它组合不影响本组合区间。
- 权益归零或转负时, 回撤深度记 100%。

## 交付物

| 文件 | 内容 |
|---|---|
| `crates/vta-analysis/` | 分析 crate: 装载、对账、统计、渲染 |
| `crates/vta-core/src/timeframe.rs` | 共享年化表 |
| `crates/vta-report/src/parquet/detail/read.rs` | detail bundle 读取与校验 |
| `crates/vta-cli/src/cli.rs` / `main.rs` | `--analyze` 第三入口与分析参数 |
| `docs/analysis.md` | 口径、公式、输出与限制 |
| `STEP.d/06-analysis.md` | 本文件 |

## 验收

```bash
cargo test -p vta-analysis
cargo clippy -p vta-analysis --all-targets -- -D warnings
cargo run -p vta --release -- --analyze reports/vta_review_smoke --out /tmp/analysis-smoke
```

验收点:

1. 单测覆盖正态 CDF/分位、回撤区间、闰年日历、PSR/DSR/MinTRL、BH、bootstrap 确定性与负权益回撤。
2. `reports/golden/current`: 对账不一致为 0; 缺 detail 的 Top 组合不再让回撤/日历表整体消失。
3. `reports/vta_review_smoke`: 52 行全部连上 detail, 4 组均算出 PBO, 组内 13 条曲线各自使用自己的 equity/trades。
4. 同一报告连续运行两次, `analysis.json`、`analysis.md` 与 `tables/*.csv` 逐字节一致。
5. `cargo fmt --all` 与 `scripts/check.sh` 通过。

## 已知限制

1. 没有 detail 的行只能参与摘要排名, 不能做 PSR/DSR/bootstrap。
2. DSR 的试验数不是独立试验数, 参数相关时偏保守。
3. BH 控制组内 FDR, 不是全报告 FWER。
4. PBO 依赖同组同网格曲线数量, 小网格会跳过。
5. 阶段 3 的多重检验与有效试验数尚未实现。
