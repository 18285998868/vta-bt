# C1 拆分 5 crate workspace

## 目标

1. 单 package 拆为 `vta-core` / `vta-strategies` / `vta-report` / `vta-pbo` / `vta-cli` 五个 crate。
2. 纯搬运: 不修改任何指标算法、成交模型、参数编码与配置语义。
3. 为 C2–C7 的逻辑重构建立可独立编译、可独立测试的边界。

## 交付物

| 路径 | 变化 |
|---|---|
| `Cargo.toml` | 虚拟清单; `members = ["crates/*"]`; 共享版本与依赖 |
| `crates/vta-core` | `error/money/engine/signal/bars/types` + `build.rs` 源码哈希 |
| `crates/vta-strategies` | 81 个策略、`Strategy`/`ParamOutcome`、`config`、`params`、`build` |
| `crates/vta-report` | Parquet 读写、schema、detail 清单与合并 |
| `crates/vta-pbo` | CSCV 与 `--pbo` 报告 |
| `crates/vta-cli` | 包名仍为 `vta`: clap、配置、编排、`app`、`data` |
| `crates/*/tests`、`crates/*/examples` | 测试与示例跟随所属 crate, `include_str!` 路径同步 |
| `docs/architecture.md` | 模块布局按 5 crate 重写 |

依赖方向:

```text
vta-cli        -> {core, strategies, report, pbo}
vta-pbo        -> {core, report, strategies}
vta-strategies -> core
vta-report     -> core (dev-dep: strategies)
```

## 验收

```bash
cargo fmt --all -- --check
./scripts/check.sh
./scripts/golden_run.sh --check
```

1. `check.sh` 全绿: fmt / check / clippy(两套 feature) / 609 nextest / 3 doctests / rustdoc / deny / audit。
2. golden 逐值一致: `result.parquet` 53 列 × 14 行, `detail/*.parquet` 42 文件。
3. `param_str` 文本、`strategy_params` 数值、逐 bar 信号与 C0 快照一致。

## 修复记录

1. `strategy_params_value("macd_12_26_9")` 在 C2 时仍未命中白名单, 已在 C3 随 62 行编码改造一并修复(见 `docs/invariants.md` 第 6 条)。
2. 路径依赖加 `version = "0.1.0"`, 否则 `cargo deny check bans` 报 wildcard dependency。
3. 删除搬运残留 `vta-report/src/parquet/params.rs`(未挂到模块树, 与 `vta-strategies/src/params.rs` 重复)。
4. rustdoc 内链改为公开可达路径(如 `crate::engine::ExecConfig`), 不链接私有子模块。
