# vector-ta (vta)

技术分析指标遍历回测器: 对多种指标/参数组合批量回测并输出报告。

## 配置

```toml
# config/config.example.toml
[data]
type = "binance"
dir = "/path/to/klines"
symbol = ["BTCUSDT", "ETHUSDT"]
timeframes = ["1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d"]

[backtest]
initial_balance = 10000.0

[output]
dir = "reports"
detail = false
overwrite = false

[[strategy]]
type = "ema"
period = { start = 1, end = 1000, step = 1 }
```

## 用法

```bash
cargo run -p vta --release
```

## License

MIT