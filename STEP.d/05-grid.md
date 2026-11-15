# C4 声明式参数网格

## 目标

1. 每个笛卡尔网格策略只在一处声明字段：结构体、展开顺序、范围校验与 `param_set` 由同一张表生成。
2. 策略侧不再手写嵌套循环与键序，`param_set` 退化为一行委托。
3. `params` 文本、`strategy_params` 编码、信号与 golden 快照逐字节不变。

## 设计

1. `crates/vta-strategies/src/grid.rs` 提供 `GridDim`、`grid_config!`、`grid_push_loops!`、`grid_retain!`、`grid_bind!`。
2. 字段行按声明顺序决定展开顺序与键序；`key` 保持历史 `params` 键名。
3. 范围校验二选一：`range (lo, hi)`（闭区间）或 `validate min`（下界/方向/步长）。
4. 过滤写 `filter (expr);`，不过滤写 `filter true;`；过滤作用于完整笛卡尔积。
5. 单字段配置用 `expand values;`，返回 `Vec<T>`，`param_set` 接收裸值引用。
6. 均线名单用 `names EXPR`，默认值用 `default "crate::config::default_*"`。
7. `code` 闭包拿到最终 `params` 文本，供 `sigma_chain` 等历史 FNV 回退族使用。

```rust
grid_config! {
    /// 策略文档。
    pub struct DemoConfig;
    fields {
        /// 布林带倍数。
        sigma: F64Param as f64, key "sigma", label "demo.sigma", range (0.1, 10.0);
        /// BB 周期。
        bblen: PeriodParam as usize, key "bblen", label "demo.bblen",
            range (2, 2000), default "crate::config::default_bblen";
    }
    expand pairs;
    code |_text| code::sigma_pair(sigma, bblen as f64);
    filter true;
    empty "demo 参数组合为空";
}
```

## 迁移

1. 42 个配置文件改用 `grid_config!`：BBRK 全家族、`aberration`、`dualthrust`、`ema_cross`、`rsi2`、`turtle`、`atrband`、`keltner`、`rbreaker`、`vwap`、`ma_atr`、`bbrk_bb`、`bbrk_max`、`bbrk_atr_ema`。
2. 26 个单均线策略继续共用 `config::MaGridCfg`，不新增字段表。
3. 以下网格保留手写入口，原因写在 `grid.rs` 模块文档与 `docs/invariants.md` 第 18 条：`macd`、`clenow`、`sgma`、`swing_fractal`、`vwap_trend`、`fiali`、`mabb_long`、`mabb_short`、`ma_slope_atr_gate`、`ssf*`。
4. `bbrk_max` 的 `bblen` 历史上没有范围校验，迁移后保持不校验；其余字段逐项对应原 `validate_range` / `validate_named`。
5. 均线名单的报错文案统一为宏格式（`未知均线 ..., 可选 a, b`），错误类型与拒绝行为不变。

## 验收

```bash
cargo test -p vta-strategies --lib
cargo test -p vta --test golden_signals
cargo clippy -p vta-strategies --all-targets -- -D warnings
./scripts/golden_run.sh --check
./scripts/check.sh
```

1. `430` 个策略库测试全绿。
2. `golden_signals` 的 `fixture_covers_every_variant` 与 `golden_snapshot_matches` 零 diff。
3. `golden_run.sh --check` 的 `params`、`strategy_params`、信号列与 detail 文件逐值一致。
