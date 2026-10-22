#!/usr/bin/env python3
"""vectorbt第三方交叉: BTCUSDT 1d SMA40 2024, 与vta audit同口径.
A: delay0 (信号当根收盘成交) vs vta audit-d0 (17笔, fixed+1781, sharpe0.161)
B: delay1 (信号次根开盘成交) vs vta audit-d1口径(零费版另行说明)
信号: prev-MA穿越 (与vta shared.rs一致), 100%复利全仓, 方向反转即反手.
"""
import glob
import os
import zipfile

import numpy as np
import pandas as pd

import vectorbt as vbt

KROOT = "/data/nutilustrader/data/binance/data/futures/um/daily/klines"


def load(symbol, tf, a, b):
    fs = sorted(glob.glob(os.path.join(KROOT, symbol, tf, "*.zip")))
    parts = []
    for f in fs:
        with zipfile.ZipFile(f) as z, z.open(z.namelist()[0]) as h:
            df = pd.read_csv(h, header=None)
        if str(df.iloc[0, 0]).strip() == "open_time":
            df = df.iloc[1:].reset_index(drop=True)
        df = df.iloc[:, [0, 1, 2, 3, 4, 5]]
        df.columns = ["ts", "open", "high", "low", "close", "vol"]
        for c in ["open", "high", "low", "close"]:
            df[c] = pd.to_numeric(df[c], errors="coerce")
        parts.append(df.dropna(subset=["close"]))
    d = pd.concat(parts, ignore_index=True)
    d["ts"] = pd.to_numeric(d["ts"], errors="coerce").astype("int64")
    d = d.sort_values("ts").drop_duplicates("ts").reset_index(drop=True)
    d["dt"] = pd.to_datetime(d["ts"], unit="ms")
    d = d[(d["dt"] >= a) & (d["dt"] < b)].reset_index(drop=True)
    d = d.set_index("dt")
    return d


d = load("BTCUSDT", "1d", "2024-01-01", "2025-01-01")
ma = d["close"].rolling(40).mean()
buy = (d["close"].shift(1) < ma.shift(1)) & (d["close"] >= ma)
sell = (d["close"].shift(1) > ma.shift(1)) & (d["close"] <= ma)
buy.iloc[:40] = False
sell.iloc[:40] = False

pf_a = vbt.Portfolio.from_signals(
    d["close"], entries=buy, exits=sell, short_entries=sell,
    short_exits=buy,
    init_cash=10000.0, size=np.inf, fees=0.0, slippage=0.0, freq="1D")
print("A delay0: trades=", len(pf_a.trades), "ret%=", round(pf_a.total_return() * 100, 2),
      "sharpe=", round(pf_a.sharpe_ratio(), 3))

pf_b = vbt.Portfolio.from_signals(
    d["open"], entries=buy.shift(1).fillna(False).astype(bool), exits=sell.shift(1).fillna(False).astype(bool),
    short_entries=sell.shift(1).fillna(False).astype(bool), short_exits=buy.shift(1).fillna(False).astype(bool),
    init_cash=10000.0, size=np.inf, fees=0.0, slippage=0.0, freq="1D")
print("B delay1-open复利: trades=", len(pf_b.trades), "ret%=", round(pf_b.total_return() * 100, 2),
      "sharpe=", round(pf_b.sharpe_ratio(), 3))

# 固定腿: 每笔固定10000名义 (size_type value), 对 vta fixed_pnl
pf_c = vbt.Portfolio.from_signals(
    d["close"], entries=buy, exits=sell, short_entries=sell,
    short_exits=buy,
    init_cash=10000.0, size=10000.0, size_type="value", fees=0.0, slippage=0.0, freq="1D")
print("C delay0固定: trades=", len(pf_c.trades), "ret%=", round(pf_c.total_return() * 100, 2),
      "sharpe=", round(pf_c.sharpe_ratio(), 3))
print("vta audit-d0: trades=17 fixed=+1781 sharpe=0.161 | audit-d1(+fee2slip1): trades=17 fixed=+1677 sharpe=0.135")
