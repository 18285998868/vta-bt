```
creats/vector-ta/src/
  main.rs 入口
  lib.rs 导出
  cli.rs 命令行参数
  error.rs thiserror库
  config.rs config-rs库
  indicator.rs vectorta指标桥接
  app.rs 向量化回测逻辑编排
  strategies/ 策略模块
    mod.rs
    ssf/
      mod.rs
      config.rs
      strategy.rs
      tests.rs

```
加载配置
for symbol 
  for tf
    加载数据
    for strategy
      参数并行
      计算指标
      策略逻辑判定
      产生Buy/Sell/Hold信号
      产生持仓,交易,策略绩效
