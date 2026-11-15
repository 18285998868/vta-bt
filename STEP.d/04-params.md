# C3 结构化参数编码

## 目标

1. `params` 文本、`strategy_params_json`、`strategy_params` 整数由**同一处**产出，不再从文本反解析。
2. 修掉 42 行 FNV-32 编码 + 8 行维度丢失 + 12 行 `SSF*` 漏 `poles`，共 **62 行**编码变更。
3. 修掉 18 行 JSON 键名撒谎（`MACD` / `VWAP` / `FIALI` / `MA_SLOPE_ATR_GATE` / `SSF*`）。
4. 信号、交易、指标、`params` 文本、`detail/*.parquet` 保持不变。

## 设计

1. 策略实现 `Strategy::param_set(idx) -> ParamSet`；`ParamSet` 同时携带 `text` / `pairs` / `code`。
2. `param_str` 降为默认方法，委托 `ParamSet::into_text()`，旧调用方无需改签名。
3. 已识别的族沿用历史槽位公式（`params::code` 中逐位复刻），未改动的行编码不变。
4. 原先走 FNV 的多键族改为定宽位打包，首字段在高位；越界回退字段哈希，启动期唯一性自检兜底。
5. 止损/仓位后缀由 `ParamSet::with_suffix` 追加：文本逐字节等于 `stop.apply_to`，编码为整串 FNV-64，后缀键值并入 JSON。
6. 删除 `strategy_params_value` / `params_to_json` 及其反解析测试，唯一性自检改查 `text` / `code`。

保留的历史回退（不在本阶段范围，编码与旧版逐位一致）：

1. `sigma_chain` 折叠值 `>= 1_000_000` 时回退 FNV-32（`bbrk_*` 两键族大周期可触发）。
2. `clenow` 任一周期 `>= 1<<15` 时回退 FNV-64。
3. 定宽打包越界时回退字段哈希；正常配置范围不会触发。

## 编码布局

| 策略族 | 布局 |
|---|---|
| `bbrk_adx` | `sigma×1000:14, len:12, adxlen:9, adx×10:10` |
| `bbrk_atr` | `sigma×1000:14, bblen:12, atr1:9, atr2:9, atrthr×1000:13` |
| `bbrk_rex` | `sigma×1000:14, bblen:12, rexn:8, rexmult×1000:15` |
| `bbrk_trail` | `sigma×1000:14, bblen:12, atrlen:9, trailk×1000:15` |
| `bbrk_ttm` | `sigma×1000:14, bblen:12, kc×1000:13, ttmn:7` |
| `bbrk_vol` / `bbrk_vol_ema` | `sigma×1000:14, bblen:12, voln:9, volmult×1000:15` |
| `macd` | `fast:21, slow:21, signal:21` |
| `sgma` | `period:17, poly_order:17` |
| `vwap` | `thresh×1000:16` |
| `fiali` | `0` |
| `ma_slope_atr_gate` | `ma:11, k:11, atr:9, kind:1, z:11, (th+10)×1000:15` |
| `ma_atr` | `name:6, len:11, mult×1000:15, atrlen:9` |
| `mabb_long` / `mabb_short` | `name:6, bblen:12, sigma×1000:14, stopk×1000:14, atrlen:9` |
| `bbrk_max` | `name:4, malen:12, bblen:16` |
| `ssf*` | `period:20, poles-2:1` |

## 验收

1. `./scripts/check.sh` 全绿（fmt / clippy 双配置 / nextest / doctest / rustdoc / deny / audit）。
2. `UPDATE_GOLDEN=1 cargo test --test golden_signals` 后逐行 review：仅 62 行变化，且只动 `params_value` / `params_json` 两列；`param_str` 与 4 个信号列不变，每行 JSON 可解析。
3. `./scripts/golden_run.sh --check`：确认 14 行结果只有 `strategy_params` / `strategy_params_json` 变化；随后 `--freeze` + `--check`，`detail/*.parquet` 42 个文件逐值一致。
4. `cargo test --test grid_uniqueness_test`：定宽族的每个声明维度都会改变编码，网格内无碰撞。

```bash
./scripts/check.sh
UPDATE_GOLDEN=1 cargo test --test golden_signals
./scripts/golden_run.sh --check
./scripts/golden_run.sh --freeze
./scripts/golden_run.sh --check
```

## 前置变更

`/data` 下两个 reader 已合并为 `data-reader` workspace（包名 `binance-core` / `dukascopy-core`），
本阶段同步了 path 依赖、crate 名与源码哈希根目录；该适配应独立于 C3 提交。
