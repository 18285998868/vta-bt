# 黄金日内 Regime-Filter 策略 — 人话版

来源：Bhatti (2026) `A Regime-Filtered Intraday Trading Framework for Gold`
标的：XAU/USD 现货，15m，NY Session 13:30-20:00 UTC，样本 2024全年247笔（仿真非实盘回测，见文末提醒）。

## 一句话
只在顺风局做回踩：200EMA定方向，VWAP定站队，50EMA上出现拒绝K才上车，止损看影线、出场只看收盘。

## 三层分工
- **200EMA = 风向标**：收盘在上只做多，在下只做空，距离<0.1%不开（边界模糊区）。
- **VWAP = 机构成本线**：多头必须站在VWAP上，空头必须在VWAP下。当日13:30 UTC重置。
- **50EMA = 入场+拖尾线**：回踩触碰后收回才算数；持有后只有收盘穿50EMA才走，盘中影线穿无视。浮盈≥2.5R后拖尾切到20EMA。

## 入场六条件（多，空镜像）
1. C1 Regime：Ct > EMA200
2. C2 VWAP：Ct > VWAP
3. C3 回踩：min(Lt,Lt-1) ≤ EMA50 ≤ Ct（碰过但收在上面）
4. C4 拒绝K：pin（下影≥2*实体 且 上影≤0.5*下影）或 牛吞没（Ct>Ot-1 且 Ot<Ct-1）
5. C5 量：Vt > 1.1*MA20(V)
6. C6 波幅：Ht-Lt ≥ 0.8*ATR14
六个同时满足，收盘下单。不追突破，只做回踩。

## VWAP管理（持仓后）
- impulsive（≤3根到VWAP且均幅>1.0ATR）：满仓拿到3R。
- compressed（≥5根≤0.6ATR挤在VWAP 0.15%内）：到VWAP先减50%，剩下用50EMA拖。
- breakout确认（收盘超VWAP 0.05%+放量）：继续拿。

## 出场
- 初损：多 `SL=Lsignal-0.5*ATR14`，空对称。唯一看影线的地方。
- 目标：3R。期望 `E=0.453*2.12-0.547*1.07≈+0.41R`。
- 拖尾：收盘破50EMA走；浮盈≥2.5R改看20EMA。当日解决，不隔夜。

## 风控
- 仓位：`N=κ*C/(ATR14*k*Vpip), κ=0.01,k=0.5`。ATR大自动变轻。例：1万刀、ATR12、pip1刀 → 约16.7微手。
- 日断路：单日-3R或连亏3笔，当天停。
- 成本：0.24R/笔（点差0.14+滑点0.05+佣金0.05）。
- 新闻：NFP/CPI/FOMC/GDP前后15m不开，已有仓把拖尾切到近期swing。

## 原文成绩（仿真）
胜45.3%，均赢2.12R，均亏-1.07R，期望+0.414R，PF1.76，总+102%，Sharpe3.99，MDD5.1%。基准EMA20/50交叉-98%。

## 重要提醒
原文6.1/7.2自认是按`满赢0.30+部分赢0.20+保本0.08+满亏0.42`参数化的Monte Carlo仿真，不是逐bar历史回测。Sharpe/MDD不可直接采信，上实盘前必须在MT5/Backtrader/QuantConnect用真实OHLCV重跑+walk-forward。

## Pine实现与原文的差异
- VWAP用13:30-2000 UTC窗口自算，`volume缺失时用1代替`，与平台tick volume会有偏差。
- 初损用`strategy.exit stop+limit`（盘中有效），拖尾用收盘`strategy.close`实现close-only。
- compressed减半用`qty_percent=50`实现一次。
- 新闻 exclusion 为手动开关，Pine拿不到财经日历。
- 日断路按UTC日期累计R实现。

## 粘贴没数据排查（v1.1已加调试面板）
1. 先看右上角调试面板：EMA200预热是否为OK（15m至少要200根以上，建议加载6个月+）。
2. inSession bar=0 → Session字符串不对：把`NY Session`改成`0000-2359:1234567`或关掉`仅NY Session交易`。
3. touch50=0 → 50EMA离价格太远，属正常，换趋势强的月份。
4. rej=0 → pin/吞没过严：打开`宽松模式`，C4只要求收阳/阴，先出信号验证链路。
5. long/short信号>0但策略无交易 → 看策略测试器日期范围、初始资金、以及是否触发日断路（红色背景）。
6. VWAP=na → 切OANDA:XAUUSD 15m，volume缺失会自动退化，不影响。
