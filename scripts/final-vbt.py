#!/usr/bin/env python3
"""最终验证: BTCUSDT 1d SMA40 2020-01-01~2026-09-29, vectorbt双腿.
口径: prev-MA穿越, 次根开盘成交, fee2bp/slip1bp单边, 初始10000.
"""
import glob
import os
import zipfile

import numpy as np
import pandas as pd

import vectorbt as vbt

KROOT = "/data/nutilustrader/data/binance/data/um/daily/klines"


def load():
    fs = sorted(glob.glob(os.path.join(KROOT, "BTCUSDT", "1d", "*.zip")))
    parts = []
    for f in fs:
        with zipfile.ZipFile(f) as z, z.open(z.namelist()[0]) as h:
            df = pd.read_csv(h, header=None)
        if str(df.iloc[0, 0]).strip() == "open_time":
            df = df.iloc[1:].reset_index(drop=True)
        df = df.iloc[:, [0, 1, 4]]
        df.columns = ["ts", "open", "close"]
        for c in ["open", "close"]:
            df[c] = pd.to_numeric(df[c], errors="coerce")
        parts.append(df.dropna(subset=["close"]))
    d = pd.concat(parts, ignore_index=True)
    d["ts"] = pd.to_numeric(d["ts"], errors="coerce").astype("int64")
    d = d.sort_values("ts").drop_duplicates("ts").reset_index(drop=True)
    d["dt"] = pd.to_datetime(d["ts"], unit="ms")
    d = d[(d["dt"] >= "2020-01-01") & (d["dt"] < "2026-09-29")].reset_index(drop=True)
    return d.set_index("dt")


d = load()
ma = d["close"].rolling(40).mean()
buy = (d["close"].shift(1) < ma.shift(1)) & (d["close"] >= ma)
sell = (d["close"].shift(1) > ma.shift(1)) & (d["close"] <= ma)
buy.iloc[:40] = False
sell.iloc[:40] = False

eb = buy.shift(1).fillna(False).astype(bool)
es = sell.shift(1).fillna(False).astype(bool)
pf = vbt.Portfolio.from_signals(
    d["open"], entries=eb, exits=es, short_entries=es, short_exits=eb,
    init_cash=10000.0, size=np.inf, fees=0.0002, slippage=0.0001, freq="1D")
print("vbt复利: trades=", len(pf.trades), "ret%=", round(pf.total_return() * 100, 2),
      "sharpe=", round(pf.sharpe_ratio(), 3),
      "wr=", round(float((pf.trades.records_readable["PnL"] > 0).mean()), 3))
pf2 = vbt.Portfolio.from_signals(
    d["open"], entries=eb, exits=es, short_entries=es, short_exits=eb,
    init_cash=10000.0, size=10000.0, size_type="value", fees=0.0002, slippage=0.0001, freq="1D")
print("vbt固定: trades=", len(pf2.trades), "ret%=", round(pf2.total_return() * 100, 2),
      "sharpe=", round(pf2.sharpe_ratio(), 3))
