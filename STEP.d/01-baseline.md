# C0 行为冻结基线

## 目标

1. 冻结三类对外契约: `param_str` 文本、`strategy_params` 编码值、逐 bar 信号。
2. 冻结端到端产物: `result.parquet` 53 列 + `detail/*.parquet` 逐值一致。
3. 后续 C1–C7 每步都以本基线为唯一"行为未变"判据。

## 交付物

| 文件 | 作用 |
|---|---|
| `crates/vta-cli/tests/golden/strategies.toml` | 81 个 `StrategyConfig` 变体各一条, 网格 2~3 档/维 |
| `crates/vta-cli/tests/golden/expected.txt` | 273 档参数 × 4 组合成行情的冻结快照 |
| `crates/vta-cli/tests/golden_signals.rs` | 快照比对 + 变体覆盖兜底 |
| `crates/vta-cli/examples/parquet_diff.rs` | parquet 逐值比较(schema/行数/f64 bit/Decimal i128) |
| `scripts/golden_run.sh` | release 冒烟 + 二跑幂等 + 基线比对 |
| `config.golden.toml` | 1d-only 冒烟负载: sma / bbrk_sma / bbrk_vol / macd |

## 使用

1. 建立基线(每台机器一次; 产物在 `reports/golden/`, 已 gitignore)。

```bash
./scripts/golden_run.sh --freeze
```

2. 重构后验收。

```bash
cargo test -p vta --test golden_signals
./scripts/golden_run.sh --check
```

3. 确认行为变更有意时, 才允许重写快照。

```bash
UPDATE_GOLDEN=1 cargo test -p vta --test golden_signals
git diff crates/vta-cli/tests/golden/expected.txt
```

## 验收

1. `fixture_covers_every_variant` 通过: 变体数 == 夹具条数(新增策略必须补夹具)。
2. `golden_snapshot_matches` 通过: 273 行逐字一致。
3. `--check` 输出 `一致: 53 列 × 14 行` 与 `detail 一致: 42 个文件`。

## 已知陷阱

1. 脚本必须 `cargo build --release --bins --examples`: 只加 `--examples` 不会重建 `target/release/vta`, 会拿旧二进制跑出"假通过"(已踩过一次)。
2. `UPDATE_GOLDEN` 只允许在人工 review diff 时设置; CI 不得设置。
3. `source_hash()` 让每次改码都全量重算, 冒烟耗时约 2 分钟属正常。
