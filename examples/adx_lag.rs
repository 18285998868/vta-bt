//! `adx_lag`: BB 突破 vs BB+ADX 进场滞后重放(研究轨机制证据, 非引擎改动)。
//!
//! 复用引擎信号: `BbrkBbStrategy`(基线) + `BbrkAdxStrategy`(过滤), 同 len/sigma。
//! 对每个基线进场 i(方向 d, 离场 e): 在 `[i..e]` 内找首个同向过滤进场 j。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use kline_reader::{DataConfig, DataSource, load_columns};
use vta::config::{F64Param, PeriodParam};
use vta::strategies::Signal;
use vta::strategies::Strategy;
use vta::strategies::bbrk_adx::{BbrkAdxConfig, BbrkAdxStrategy};
use vta::strategies::bbrk_bb::{BbrkBbConfig, BbrkBbStrategy};

fn quantile(mut v: Vec<i64>, q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_unstable();
    let pos = q * (v.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        v[lo] as f64
    } else {
        v[lo] as f64 * (hi as f64 - pos) + v[hi] as f64 * (pos - lo as f64)
    }
}

fn median_f(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    // `total_cmp` 而非 `partial_cmp().unwrap()`: 序列里混入 NaN 时后者会 panic。
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn run_pair(symbol: &str, timeframe: &str, len: usize, sigma: f64, adxlen: usize, thr: f64) {
    // 数据根目录: 优先取环境变量, 否则回落到本机的默认数据目录。
    // 硬编码绝对路径会让 example 换台机器就失效。
    let data_dir =
        std::env::var("VTA_DATA_DIR").unwrap_or_else(|_| "/data/nutilustrader/data".to_string());
    let cfg = DataConfig::new(
        DataSource::Binance,
        &data_dir,
        symbol,
        timeframe,
        Some("2020-09-15"),
        Some("2025-09-30"),
    )
    .expect("valid DataConfig");
    let cols = load_columns(&cfg).expect("load_columns");
    let n = cols.len();

    let base = BbrkBbStrategy::new(BbrkBbConfig {
        ma: vec!["sma".to_string()],
        bblen: PeriodParam::Single(len),
        sigma: F64Param::Single(sigma),
    });
    let filt = BbrkAdxStrategy::new(BbrkAdxConfig {
        sigma: F64Param::Single(sigma),
        bblen: PeriodParam::Single(len as f64 as usize),
        adxlen: PeriodParam::Single(adxlen),
        adx: F64Param::Single(thr),
    });
    let bs = base
        .generate_signals_full(
            cols.open(),
            cols.high(),
            cols.low(),
            cols.close(),
            cols.volume(),
            cols.time(),
            0,
        )
        .expect("base signals");
    let fs = filt
        .generate_signals_full(
            cols.open(),
            cols.high(),
            cols.low(),
            cols.close(),
            cols.volume(),
            cols.time(),
            0,
        )
        .expect("filt signals");

    // 基线交易: (entry, exit, dir)
    let mut trades: Vec<(usize, usize, i8)> = Vec::new();
    let mut i = 0;
    while i < n {
        match bs[i] {
            Signal::Buy | Signal::Sell => {
                let d = if bs[i] == Signal::Buy { 1 } else { -1 };
                let mut e = n - 1;
                for (k, &sig_k) in bs.iter().enumerate().skip(i + 1) {
                    if sig_k == Signal::Close {
                        e = k;
                        break;
                    }
                    // 基线同向再入场不应出现(持仓中无Buy/Sell); 若有则截断
                    if sig_k == Signal::Buy || sig_k == Signal::Sell {
                        e = k;
                        break;
                    }
                }
                trades.push((i, e, d));
                i = e + 1;
            }
            _ => i += 1,
        }
    }

    let mut lags: Vec<i64> = Vec::new();
    let mut slips: Vec<f64> = Vec::new();
    let mut lag0 = 0usize;
    let mut missed = 0usize;
    let mut missed_win = 0usize; // 错过单中基线excursion收益为正者
    let mut missed_rets: Vec<f64> = Vec::new();
    let mut lagpos: Vec<i64> = Vec::new();
    let mut slippos: Vec<f64> = Vec::new();
    for (bi, be, d) in &trades {
        let want = if *d == 1 { Signal::Buy } else { Signal::Sell };
        let mut hit: Option<usize> = None;
        for (j, &sig_j) in fs.iter().enumerate().take(*be + 1).skip(*bi) {
            if sig_j == want {
                hit = Some(j);
                break;
            }
        }
        match hit {
            Some(j) => {
                lags.push((j - bi) as i64);
                if j == *bi {
                    lag0 += 1;
                } else {
                    lagpos.push((j - bi) as i64);
                }
                let slip = if *d == 1 {
                    (cols.close()[j] - cols.close()[*bi]) / cols.close()[*bi] * 10000.0
                } else {
                    (cols.close()[*bi] - cols.close()[j]) / cols.close()[*bi] * 10000.0
                };
                slips.push(slip);
                if j != *bi {
                    slippos.push(slip);
                }
            }
            None => {
                missed += 1;
                let r = if *d == 1 {
                    (cols.close()[(*be).min(n - 1)] - cols.close()[*bi]) / cols.close()[*bi]
                        * 10000.0
                } else {
                    (cols.close()[*bi] - cols.close()[(*be).min(n - 1)]) / cols.close()[*bi]
                        * 10000.0
                };
                missed_rets.push(r);
                if r > 0.0 {
                    missed_win += 1;
                }
            }
        }
    }
    let matched = trades.len() - missed;
    println!(
        "{} {} len={} sigma={} adxlen={} thr={}: bars={} base_trades={} matched={} missed={} miss_rate={:.1}% lag0_share={:.1}% lag_med={:.0} lag_p90={:.0} slip_med={:.1}bps | lagged_med={:.0} lagged_p90={:.0} lagged_slip_med={:.1}bps missed_win_rate={:.1}% missed_ret_med={:.1}bps",
        symbol,
        timeframe,
        len,
        sigma,
        adxlen,
        thr,
        n,
        trades.len(),
        matched,
        missed,
        missed as f64 / trades.len().max(1) as f64 * 100.0,
        lag0 as f64 / matched.max(1) as f64 * 100.0,
        quantile(lags.clone(), 0.5),
        quantile(lags.clone(), 0.9),
        median_f(slips),
        quantile(lagpos.clone(), 0.5),
        quantile(lagpos.clone(), 0.9),
        median_f(slippos),
        missed_win as f64 / missed.max(1) as f64 * 100.0,
        median_f(missed_rets),
    );
}

fn main() {
    // README原口径: len20 sigma2 adxlen14 thr25; 附thr20对照
    for (tf,) in [("1h",), ("4h",)] {
        run_pair("BTCUSDT", tf, 20, 2.0, 14, 25.0);
    }
    for (tf,) in [("1h",), ("4h",)] {
        run_pair("BTCUSDT", tf, 20, 2.0, 14, 20.0);
    }
}
