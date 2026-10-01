#!/usr/bin/env python3
"""vta逐笔审计: 用独立Python实现重算detail parquet前3笔, 核对进出价与fixed_pnl.

约定与引擎一致 (03_run_backtest.rs):
- 信号序列由引擎生成, 此处只验算"信号->成交"一段: 取每笔 entry_time/exit_time
  定位 bar, 按 delay/fee/slip 重算期望进出价; 再按 fixed 名义重算 fixed_pnl。
- delay=0: 当根 close; delay>0: fill bar open; 买抬价/卖压价; 佣金按单边名义扣。
- 注意: entry_time 即 fill bar 时间戳, 故 entry_bar = time_index(entry_time)。

用法:
  python3 audit_fills.py --detail <detail.parquet> --symbol BTCUSDT --timeframe 1d
      --klines /data/.../klines --delay 1 --fee 2 --slip 1 --balance 10000
退出码 0=全过, 1=有差异。
"""
import argparse
import glob
import os
import sys
import zipfile

import pandas as pd


def load_klines(root, symbol, tf):
    fs = sorted(glob.glob(os.path.join(root, symbol, tf, "*.zip")))
    parts = []
    for f in fs:
        with zipfile.ZipFile(f) as z:
            name = z.namelist()[0]
            with z.open(name) as h:
                df = pd.read_csv(h, header=None)
        if str(df.iloc[0, 0]).strip() == "open_time":
            df = df.iloc[1:].reset_index(drop=True)
        df = df.iloc[:, [0, 1, 2, 3, 4]]
        df.columns = ["ts", "open", "high", "low", "close"]
        for c in ["open", "high", "low", "close"]:
            df[c] = pd.to_numeric(df[c], errors="coerce")
        parts.append(df.dropna(subset=["close"]))
    alld = pd.concat(parts, ignore_index=True)
    alld["ts"] = pd.to_numeric(alld["ts"], errors="coerce").astype("int64")
    return alld.sort_values("ts").drop_duplicates("ts").reset_index(drop=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--detail", required=True)
    ap.add_argument("--symbol", required=True)
    ap.add_argument("--timeframe", required=True)
    ap.add_argument("--klines", required=True)
    ap.add_argument("--delay", type=int, default=0)
    ap.add_argument("--fee", type=float, default=0.0)
    ap.add_argument("--slip", type=float, default=0.0)
    ap.add_argument("--balance", type=float, default=10000.0)
    ap.add_argument("--n", type=int, default=3)
    a = ap.parse_args()

    det = pd.read_parquet(a.detail)
    kl = load_klines(a.klines, a.symbol, a.timeframe)
    tsidx = {int(t): i for i, t in enumerate(kl["ts"].values)}
    fr = a.fee / 10000.0
    sr = a.slip / 10000.0
    ok = True
    for k in range(min(a.n, len(det))):
        t = det.iloc[k]
        side = 1 if t["side"] == "LONG" else -1
        ei, xi = tsidx[int(t["entry_time"])], tsidx[int(t["exit_time"])]
        e_base = float(kl["close"].values[ei]) if a.delay == 0 else float(kl["open"].values[ei])
        x_base = float(kl["close"].values[xi]) if a.delay == 0 else float(kl["open"].values[xi])
        e_exp = e_base * (1 + sr) if side == 1 else e_base * (1 - sr)
        x_exp = x_base * (1 - sr) if side == 1 else x_base * (1 + sr)
        fq = a.balance / e_exp
        fp_exp = side * (x_exp - e_exp) * fq - (e_exp + x_exp) * fq * fr
        de = abs(t["entry_price"] - e_exp)
        dx = abs(t["exit_price"] - x_exp)
        df_ = abs(t["fixed_pnl"] - fp_exp)
        passed = de < 1e-6 and dx < 1e-6 and df_ < 1e-4
        ok &= passed
        print(f"trade{k} side={t['side']} bars={t['bars_held']} "
              f"d_entry={de:.2e} d_exit={dx:.2e} d_fixed={df_:.2e} "
              f"{'PASS' if passed else 'FAIL'}")
    print("AUDIT_ALL_PASS" if ok else "AUDIT_FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
