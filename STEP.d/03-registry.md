# C2 策略注册表单表化

## 目标

1. `StrategyConfig` 的 81 个变体只有一个声明处: `crates/vta-strategies/src/registry.rs` 的 `strategy_table!`。
2. 新增策略从改 4 处手写清单降为**加一行表项** + 一条 golden 夹具。
3. 枚举变体、TOML tag、构造入口、参数校验、模块声明由同一张表生成，不会漂移。

## 设计

表列: `(模块, TOML tag, 枚举变体, 配置模块, 配置类型, 策略类型, 构造方式)`。

```text
(sma, "sma", Sma, config, MaGridCfg, SmaStrategy, period)
(aberration, "aberration", Aberration, aberration, AberrationConfig, AberrationStrategy, clone)
```

三个消费者:

| 位置 | 生成内容 |
|---|---|
| `vta-strategies/src/lib.rs` | `pub mod <strategy>` + 模块文档 |
| `vta-strategies/src/config.rs` | `StrategyConfig` 枚举（显式 `#[serde(rename = tag)]`）、`validate()`、`STRATEGY_TAGS` |
| `vta-strategies/src/build.rs` | 穷尽 `build_one`（`period` / `clone` 两种构造） |

`vta-cli/src/config/mod.rs` 的 81 臂 `validate` match 改为调用 `spec.strategy.validate()?`。

## 验收

```bash
./scripts/check.sh
./scripts/golden_run.sh --check
```

1. `check.sh` 全绿: fmt / clippy 双配置 / **607 tests** / 3 doctests / rustdoc / deny / audit。
2. golden 逐值一致: `53 列 × 14 行`, `detail 42 个文件`。
3. `STRATEGY_TAGS` 与 81 条 golden 夹具的 `type` 完全一致（`fixture_covers_every_variant`）。

## 测试调整

1. `fixture_covers_every_variant` 改读 `STRATEGY_TAGS`，不再正则解析 `config.rs` 源码。
2. 删除 `build_coverage_test.rs` 中两条解析源码文本的测试（枚举与构造臂同源后失去意义），保留真实配置端到端构建测试。
3. 测试数 609 → 607；无行为覆盖损失（golden 快照仍逐档构建全部 81 个策略）。

## 冻结契约

1. `TOML type` 文本在表里写死，与 C0 夹具逐一核对；重构不得改动。
2. `param_str`、`strategy_params`、信号指纹由 golden 快照继续冻结。
