# PBO 模式

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

