# 配置参考

```toml
[data]
type = "binance" # 必填，"binance" | "dukascopy"
market = "futures/um" # 可选，Binance 市场："spot" | "futures/um" | "futures/cm"，默认 futures/um
dir = "path/to/data" # 必填，数据根目录（自动追加数据源子路径）
symbol = "BTCUSDT" # 必填，单值或数组
timeframe = "1d" # 必填，单值或数组
# start_date = "2024-01-01" # 可选
# end_date = "2024-12-31" # 可选
[backtest]
initial_balance = 10000.0 # 必填，>0
# delay_bars: 可选, 默认 1, **必须 >= 1**(0 是前视乐观口径, 反序列化期即被拒)
# fill_bar_price: 可选, "open"(默认) | "close"
# fee_bps / slippage_bps: 可选, 默认 0, 非负且有限
[output]
dir = "reports/vta" # 可选，默认 "reports"
detail = false # 可选，默认 false
fixed_curves = false # 可选，默认 false；仅当 detail = true 时允许
overwrite = false # 可选，默认 false
[[strategy]] # 必填，至少一个
type = "sma"
period = { start = 5, end = 100, step = 5 } # 范围或单值
# 可选：该条策略专属的持仓期风险规则（缺省关闭）。
# 同一策略可重复写多条 [[strategy]]，每条配不同 stop，即形成止损网格。
# stop = { stop_loss_pct = 2.0, trailing_stop_pct = 5.0 }
# stop = { atr = { period = 14, mult = 2.0, trailing = true } }
# stop = { breakeven = { trigger_pct = 2.0, offset_pct = 0.0 } }
# 也可写成表：先写 [[strategy]]，再写 [strategy.stop] 与对应键。
```

- 未知键拒收；读写/解析失败带文件路径。
- 全量示例见 `config.example.toml`；`config.ma_all_1h.toml` / `config.sma_100.toml` 为常用组合；`scripts/` 下有定稿配置。

### 持仓期止损（引擎级 `stop`）

`stop` 是**每条 `[[strategy]]` 独立**的运行参数，不是全局开关，也不属于策略代码。
它只作用于引擎已成交的持仓，策略信号数组本身不变；但止损平仓后引擎会把目标
方向重置为 `Flat`（见下），因此后续 `Hold` 不会自动重入。

- 固定百分比止损：多头 `entry × (1 - stop_loss_pct/100)`，空头
  `entry × (1 + stop_loss_pct/100)`。
- 百分比追踪止损：多头 `extreme × (1 - trailing_stop_pct/100)`，空头
  `extreme × (1 + trailing_stop_pct/100)`；`extreme` 是入场后的最高 high /
  最低 low，**在每根 bar 收盘后更新，供下一根 bar 判定**。
- ATR 止损：`atr = { period = 14, mult = 2.0, trailing = false }`。ATR 用
  Wilder 平滑；固定 ATR 止损在入场时确定，追踪 ATR 止损在每根收盘后用
  `extreme ∓ mult × ATR` 更新。开盘成交取上一根已收盘 bar 的 ATR，收盘成交
  取入场 bar 的 ATR；追踪更新同样只用已收盘 bar 的 ATR，下一根生效。追踪
  止损**只收紧不放松**：ATR 放大时保留上一根更紧的止损。
- 保本止损：`breakeven = { trigger_pct = 2.0, offset_pct = 0.0 }`。收盘价相对
  入场价的有利幅度达到 `trigger_pct` 后，把止损移到入场价加减 `offset_pct`；
  收盘后更新，下一根 bar 生效，不使用当根 high/low 做前视判定。
- 多条规则同时配置时取更紧的一侧（多头取较大值，空头取较小值）。固定百分比/
  ATR 固定止损记为 `stop_loss`；百分比/ATR 追踪与保本止损记为
  `trailing_stop`。
- 百分比字段都必须严格位于 `(0, 100)` 内；ATR `period >= 1`、`mult > 0`；
  保本 `trigger_pct ∈ (0, 100)`、`offset_pct ∈ [0, 100)`。非法值在配置校验期
  即被拒绝。
- 盘中用 `high/low` 判定触发：多头 `low <= stop`、空头 `high >= stop`。
  正常按止损价成交；若开盘已跳空穿过止损（多头 `open <= stop`、空头
  `open >= stop`），按 `open` 成交。之后仍按 `ExecConfig` 扣逆向滑点与双边佣金。
- `fill_bar_price = "open"`（默认）：先执行到期的开盘成交，再在同一根 bar
  检查止损，因此开盘新仓当根就可能被止损。
- `fill_bar_price = "close"`：先检查盘中止损，再执行到期的收盘成交，避免用
  收盘价“跳过”盘中已经发生的止损。
- 收盘价入场的那根 bar 不回溯使用该 bar 的 high/low；开盘价入场则可以当根触发。
- 止损平仓后目标方向重置为 `Flat`，并作废止损前已经排队的目标；后续 `Hold`
  不会自动重入，只有新的 `Buy` / `Sell` 信号才会在 `delay_bars` 后重新入场。
- 未配置 `stop` 且策略不提供止损线时，引擎完全不读 high/low，结果与旧路径逐位
  一致；`delay_bars >= 1` 的口径也不变。

同一策略配多个止损档位时，重复 `[[strategy]]` 即可。报告 `params` 会追加
`;stop_loss_pct=...` / `;trailing_stop_pct=...` / `;atr_period=...` /
`;breakeven_trigger_pct=...` 等后缀，缓存指纹包含 stop，报告有
`stop_loss_exits` / `trailing_stop_exits` / `strategy_stop_exits` 三列
（整数计数）。

### 策略出场与策略止损线

引擎支持三层互相独立的离场：

1. **策略出场（收盘确认）**：策略在信号 bar 输出 `Close`（双边平仓）、
   `ExitLong`（仅平多）或 `ExitShort`（仅平空）。它们在 `delay_bars` 后的
   fill bar 按开盘/收盘口径成交，平仓原因记为 `signal`。`ExitLong` 在目标
   为空头或空仓时保持目标不变，`ExitShort` 反之。
2. **策略止损线（盘中触发）**：策略实现 `Strategy::generate_stop_lines`，
   输出 `StopLines { long, short }` 两条逐 bar 价格线。引擎在每根 bar 的
   `low/high` 触发时成交，原因记为 `strategy_stop`，并计入
   `strategy_stop_exits`。价格线必须覆盖信号长度；`NaN` / `0` / 负数表示该
   bar 不设止损。**无前视契约**：`long[i]` / `short[i]` 只能使用第 `i` 根
   bar 开始前已知的信息。
3. **引擎风险规则（`stop`）**：百分比/ATR/保本规则，由引擎计算并成交，见上。

策略止损线与 `stop` 可同时启用，引擎取更紧的一侧；策略止损线不改变策略
信号数组本身，但止损成交后与引擎止损一样会把目标方向重置为 `Flat` 并作废
已排队目标，后续 `Hold` 不会自动重入。

> **旧 `_sl` 已删除**：`bbrk_hma_v1_sl` … `bbrk_hma_v5_sl` 已被引擎 stop 取代，
> 并从策略注册表/源码中移除，旧报告中的这些策略名不再可配置。旧实现只作用于
> `pos=2` 的逆势腿，距离用入场根冻结的 `stopk × ATR`，且按收盘价判定，与这里的
> “全持仓、盘中 high/low（百分比/ATR/保本）”口径**不同**，历史结果不可直接对比。需要止损时
> 请使用对应基础策略 + 引擎 stop（百分比/ATR/保本），或由策略实现
> `generate_stop_lines` 输出自己的价格线，不要再新增 `_sl` 变体。

