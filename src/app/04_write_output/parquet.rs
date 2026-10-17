use std::path::PathBuf;
use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field};
use arrow::record_batch::RecordBatch;

use crate::app::RunResult;
use crate::engine::SummaryMetrics;
use crate::error::Result;

/// 参数串 → JSON 对象字符串。**必须产出合法 JSON**(下游整列解析)。
///
/// `param_str` 的形态分两类, 值一律按 **JSON 字符串**输出并转义:
/// - `k=v,k=v` 形态: 键即参数名(`{"sigma":"2.5","bblen":"20"}`)
/// - `k=v,variant=Name` 形态: 值含非数字枚举(`{"period":"20","variant":"Three"}`)
///
/// 历史缺陷: 数字形态原样内插输出 `{"sigma":2.5}`, 而字符串形态(`ma=hma` /
/// `variant=`)输出裸 token `{"ma":hma}` —— **非法 JSON 且与同列内合法值并存**,
/// 使整列无法被 JSON 工具消费。
pub fn params_to_json(params: &str) -> String {
    let clean = params.replace(' ', "");
    // 双均线 "fast=F,slow=S" → {"fast":"F","slow":"S"}。
    if let Some(rest) = clean.strip_prefix("fast=") {
        let mut it = rest.split(',');
        let f = it.next().unwrap_or("0");
        let s = it
            .next()
            .unwrap_or("0")
            .strip_prefix("slow=")
            .unwrap_or("0");
        return format!("{{\"fast\":{},\"slow\":{}}}", json_str(f), json_str(s));
    }
    // 新策略 param_str 原样转 JSON 键(键名即含义, 各策略内唯一)。
    for prefix in [
        "entry=", "n=", "stop=", "k=", "sigma=", "buy=", "reset=", "ma=",
    ] {
        if clean.strip_prefix(prefix).is_some() {
            let mut parts = Vec::new();
            for kv in clean.split(',') {
                let mut it = kv.splitn(2, '=');
                if let (Some(k), Some(v)) = (it.next(), it.next()) {
                    parts.push(format!("\"{}\":{}", k, json_str(v)));
                }
            }
            return format!("{{{}}}", parts.join(","));
        }
    }
    if let Some(rest) = clean.strip_prefix("period=") {
        let period = rest.split(',').next().unwrap_or("0");
        if let Some(variant_pos) = rest.find(",variant=") {
            let variant = &rest[variant_pos + 9..];
            format!(
                "{{\"period\":{},\"variant\":{}}}",
                json_str(period),
                json_str(variant)
            )
        } else {
            format!("{{\"period\":{}}}", json_str(period))
        }
    } else {
        // MA strategies: "ema_3", "sma_10", etc. — extract digits after last '_'
        let period = clean
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        format!("{{\"period\":{}}}", json_str(&period))
    }
}

/// 极简 JSON 字符串字面量: 加引号 + 转义 `"` `\\` 与控制字符。
/// 参数串只含 ASCII 键值与少量符号, 故不引入 serde_json 运行时依赖。
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// FNV-1a 32（编码槽位溢出与未识别格式时的回退口径，确定性；见 `strategy_params_value`）。
fn fnv1a_32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub fn strategy_params_value(params: &str) -> u64 {
    let clean = params.replace(' ', "");
    // 已知形态白名单前缀。走到最后的分支意味着参数串形态未被识别。
    // 该形态不在白名单里时走 FNV 回退 + warn, 而不是静默编码成"末尾数字"
    // (后者在 `period=abc` 之类输入下会退化成 0, 与真实的 period=0 撞值)。
    let known = [
        "fast=", "entry=", "n=", "ma=", "stop=", "k=", "sigma=", "buy=", "reset=", "period=",
    ];
    let _ = &known; // 白名单仅用于可读性提示; 分支本身已逐个覆盖
    // 双均线 "fast=F,slow=S" 编码为 F*1_000_000+S(F/S<1_000_000, 本项目网格内唯一)。
    if let Some(rest) = clean.strip_prefix("fast=") {
        let mut it = rest.split(',');
        let f: u64 = it.next().unwrap_or("0").parse().unwrap_or(0);
        let s: u64 = it
            .next()
            .unwrap_or("0")
            .strip_prefix("slow=")
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        return f.saturating_mul(1_000_000).saturating_add(s);
    }
    // 海龟 "entry=E,exit=X" → E*1M+X。
    if let Some(rest) = clean.strip_prefix("entry=") {
        let mut it = rest.split(',');
        let e: u64 = it.next().unwrap_or("0").parse().unwrap_or(0);
        let x: u64 = it
            .next()
            .unwrap_or("0")
            .strip_prefix("exit=")
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        return e.saturating_mul(1_000_000).saturating_add(x);
    }
    // "n=N,k=K"(DualThrust)/"n=N,m=M"(Aberration) → N*1M+round(次值*1000)。
    if let Some(rest) = clean.strip_prefix("n=") {
        let mut it = rest.split(',');
        let n: u64 = it.next().unwrap_or("0").parse().unwrap_or(0);
        let kv = it.next().unwrap_or("0");
        let second: u64 = kv
            .split('=')
            .nth(1)
            .unwrap_or("0")
            .parse::<f64>()
            .map(|v| (v * 1000.0).round() as u64)
            .unwrap_or(0);
        return n.saturating_mul(1_000_000).saturating_add(second);
    }
    // BBRK_MAX "ma=NAME,malen=L" → idx(NAME)*1M+L(idx按 MA_NAMES 顺序)。各策略内唯一。
    // BBRK_BB "ma=NAME,len=L,sigma=S" → idx(NAME)*10M+L*10k+round(S*1000)(idx按 BB_MA_NAMES 顺序)。
    if let Some(rest) = clean.strip_prefix("ma=") {
        if rest.contains("malen=") {
            let mut it = rest.split(',');
            let name = it.next().unwrap_or("");
            let len: u64 = it
                .next()
                .unwrap_or("0")
                .split('=')
                .nth(1)
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0);
            let idx = crate::strategies::bbrk_max::MA_NAMES
                .iter()
                .position(|&m| m == name)
                .unwrap_or(0) as u64;
            return idx.saturating_mul(1_000_000).saturating_add(len);
        }
        let mut len: u64 = 0;
        let mut sig: u64 = 0;
        let name = rest.split(',').next().unwrap_or("");
        for kv in rest.split(',').skip(1) {
            let mut kvit = kv.splitn(2, '=');
            match (kvit.next().unwrap_or(""), kvit.next().unwrap_or("0")) {
                ("len", v) => len = v.parse::<u64>().unwrap_or(0),
                ("sigma", v) => {
                    sig = v
                        .parse::<f64>()
                        .map(|f| (f * 1000.0).round() as u64)
                        .unwrap_or(0)
                }
                _ => {}
            }
        }
        let idx = crate::strategies::bbrk_bb::BB_MA_NAMES
            .iter()
            .position(|&m| m == name)
            .unwrap_or(0) as u64;
        return idx
            .saturating_mul(10_000_000)
            .saturating_add(len.saturating_mul(10_000))
            .saturating_add(sig);
    }
    // R-Breaker "stop=S" → round(S*100); ATR带/Keltner "k=K" → round(K*1000);
    // BBMR "sigma=S" → round(S*1000)。各策略内唯一。
    if let Some(rest) = clean.strip_prefix("stop=") {
        return rest
            .parse::<f64>()
            .map(|v| (v * 100.0).round() as u64)
            .unwrap_or(0);
    }
    if let Some(rest) = clean.strip_prefix("k=") {
        return rest
            .parse::<f64>()
            .map(|v| (v * 1000.0).round() as u64)
            .unwrap_or(0);
    }
    if let Some(rest) = clean.strip_prefix("sigma=") {
        // BBRK_ADX "sigma=S,adx=T" / BBRK_TRAIL "sigma=S,trailk=K"
        // → round(S*1000)*1M+round(T*10)，各策略内唯一。
        // 三键形如 "sigma=S,kc=K,ttmn=N"(BBRK_TTM) 或 "sigma=S,voln=N,volmult=M"(BBRK_VOL)：
        // round(S*100)*1M + 后续键折叠(h=h*1000+slot，整数键直接取值、小数键×10)，
        // 各策略网格内唯一（strategy 列隔离跨策略）。
        // 溢出修正（2026-10-12）：四键起 h 可 ≥1M（如 VOL 的 bblen 格），旧式
        // s*1M+h 会跨 s 槽碰撞（如 sigma=1.5/bblen=60 ≡ sigma=2/bblen=10）。
        // h<1M 走旧口径（全部封存编码逐位不变）；h≥1M 走 FNV 回退。
        if rest.matches(',').count() >= 2 {
            let mut it = rest.split(',');
            let s: u64 = it
                .next()
                .unwrap_or("0")
                .parse::<f64>()
                .map(|v| (v * 100.0).round() as u64)
                .unwrap_or(0);
            let mut h: u64 = 0;
            for kv in it {
                let v = kv.split('=').nth(1).unwrap_or("0");
                let slot: u64 = v.parse::<u64>().unwrap_or_else(|_| {
                    v.parse::<f64>()
                        .map(|f| (f * 10.0).round() as u64)
                        .unwrap_or(0)
                });
                h = h.saturating_mul(1000).saturating_add(slot);
            }
            if h < 1_000_000 {
                return s.saturating_mul(1_000_000).saturating_add(h);
            }
            return u64::from(fnv1a_32(&clean));
        }
        if let Some(comma) = rest.find(',') {
            let s: u64 = rest[..comma]
                .parse::<f64>()
                .map(|v| (v * 1000.0).round() as u64)
                .unwrap_or(0);
            let t: u64 = rest[comma + 1..]
                .split('=')
                .nth(1)
                .unwrap_or("0")
                .parse::<f64>()
                .map(|v| (v * 10.0).round() as u64)
                .unwrap_or(0);
            // 纯 sigma 形如 "2.5" 无逗号才走旧口径；含逗号但解析失败则回落 0(测试锁定)。
            if s != 0 || t != 0 {
                return s.saturating_mul(1_000_000).saturating_add(t);
            }
            return 0;
        }
        return rest
            .parse::<f64>()
            .map(|v| (v * 1000.0).round() as u64)
            .unwrap_or(0);
    }
    // RSI2 "buy=B,sell=S" → round(B*10)*10000+round(S*10)。
    if let Some(rest) = clean.strip_prefix("buy=") {
        let mut it = rest.split(',');
        let b: u64 = it
            .next()
            .unwrap_or("0")
            .parse::<f64>()
            .map(|v| (v * 10.0).round() as u64)
            .unwrap_or(0);
        let s: u64 = it
            .next()
            .unwrap_or("0")
            .strip_prefix("sell=")
            .unwrap_or("0")
            .parse::<f64>()
            .map(|v| (v * 10.0).round() as u64)
            .unwrap_or(0);
        return b.saturating_mul(10_000).saturating_add(s);
    }
    // VWAP_TREND "reset=M"(0=连续24h,1=UTC日切) → M。各策略内唯一。
    if let Some(rest) = clean.strip_prefix("reset=") {
        return rest
            .split(',')
            .next()
            .unwrap_or("0")
            .parse::<u64>()
            .unwrap_or(0);
    }
    if let Some(rest) = clean.strip_prefix("period=") {
        rest.split(',')
            .next()
            .unwrap_or("0")
            .parse::<u64>()
            .unwrap_or(0)
    } else {
        // MA strategies: "ema_3", "sma_10", etc. —— 取末尾数字作 period。
        let digits: String = clean
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        match digits.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                // 形态未识别: 必须可见。静默返 0 会与"真实 period=0"撞值,
                // 使报告里两个不同参数在 strategy_params 列上不可区分。
                tracing::warn!(
                    params,
                    "未识别的参数格式, strategy_params 退化为 FNV-32; 排序代理失效但 params 列仍可区分"
                );
                u64::from(fnv1a_32(&clean))
            }
        }
    }
}

fn build_parquet(results: &[RunResult], balance: f64) -> RecordBatch {
    let mut strategy = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut timeframe = StringBuilder::new();
    let mut strategy_params = UInt64Builder::new();
    let mut strategy_params_json = StringBuilder::new();
    let mut params_full = StringBuilder::new();
    let mut bal = Float64Builder::new();
    let mut trade_size = StringBuilder::new();
    let mut pnl = Float64Builder::new();
    let mut eq = Float64Builder::new();
    let mut mdd = Float64Builder::new();
    let mut trades = UInt32Builder::new();
    let mut sharpe = Float64Builder::new();
    let mut sortino = Float64Builder::new();
    let mut wr = Float64Builder::new();
    let mut pf = Float64Builder::new();
    let mut calmar = Float64Builder::new();
    let mut return_mdd = Float64Builder::new();
    let mut avg_win = Float64Builder::new();
    let mut avg_loss = Float64Builder::new();
    let mut expectancy = Float64Builder::new();
    let mut consecutive_wins = UInt32Builder::new();
    let mut consecutive_losses = UInt32Builder::new();
    let mut volatility = Float64Builder::new();
    let mut recovery_factor = Float64Builder::new();
    let mut avg_hold_bars = Float64Builder::new();
    let mut avg_mae = Float64Builder::new();
    let mut avg_mfe = Float64Builder::new();
    let mut edge_ratio = Float64Builder::new();
    let mut wl_ratio = Float64Builder::new();
    let mut skewness = Float64Builder::new();
    let mut kurtosis = Float64Builder::new();
    let mut sqn = Float64Builder::new();
    let mut pf_stability = Float64Builder::new();
    let mut z_score = Float64Builder::new();
    let mut p_value = Float64Builder::new();
    let mut fixed_pnl = Float64Builder::new();
    let mut turnover_per_year = Float64Builder::new();
    let mut delay_bars = UInt32Builder::new();
    let mut fee_bps = Float64Builder::new();
    let mut slippage_bps = Float64Builder::new();
    let mut fixed_total_return = Float64Builder::new();
    let mut fixed_sharpe_ratio = Float64Builder::new();
    let mut fixed_max_drawdown_pct = Float64Builder::new();
    let mut fixed_expectancy_bps = Float64Builder::new();
    let mut fixed_profit_factor = Float64Builder::new();
    let mut fill_close = UInt8Builder::new();
    let mut buy_hold_pnl = Float64Builder::new();
    let mut buy_hold_return = Float64Builder::new();
    let mut bars_used = UInt32Builder::new();
    let mut run_status = StringBuilder::new();

    for r in results {
        let m = &r.metrics;
        strategy.append_value(&r.strategy);
        symbol.append_value(&r.symbol);
        timeframe.append_value(&r.timeframe);
        // 原样写出 RunResult 携带的值: 合并时不再从 params 重新推导。
        strategy_params.append_value(r.strategy_params);
        strategy_params_json.append_value(&r.strategy_params_json);
        params_full.append_value(&r.params);
        bal.append_value(balance);
        trade_size.append_value("100%");
        pnl.append_value(m.total_return);
        eq.append_value(m.final_equity);
        mdd.append_value(m.max_drawdown_pct / 100.0);
        trades.append_value(m.num_trades);
        sharpe.append_value(m.sharpe_ratio);
        sortino.append_value(m.sortino_ratio);
        wr.append_value(m.win_rate / 100.0);
        pf.append_value(m.profit_factor);
        calmar.append_value(m.calmar_ratio);
        let rmdd = if m.max_drawdown_pct > 0.0 {
            m.total_return_pct / m.max_drawdown_pct
        } else {
            0.0
        };
        return_mdd.append_value(rmdd);
        avg_win.append_value(m.avg_win);
        avg_loss.append_value(m.avg_loss);
        expectancy.append_value(m.expectancy);
        consecutive_wins.append_value(m.consecutive_wins);
        consecutive_losses.append_value(m.consecutive_losses);
        volatility.append_value(m.volatility);
        recovery_factor.append_value(m.recovery_factor);
        avg_hold_bars.append_value(m.avg_hold_bars);
        avg_mae.append_value(m.avg_mae);
        avg_mfe.append_value(m.avg_mfe);
        edge_ratio.append_value(m.edge_ratio);
        wl_ratio.append_value(m.wl_ratio);
        skewness.append_value(m.skewness);
        kurtosis.append_value(m.kurtosis);
        sqn.append_value(m.sqn);
        pf_stability.append_value(m.pf_stability);
        z_score.append_value(m.z_score);
        p_value.append_value(m.p_value);
        fixed_pnl.append_value(m.total_fixed_pnl);
        turnover_per_year.append_value(m.turnover_per_year);
        delay_bars.append_value(r.delay_bars);
        fee_bps.append_value(r.fee_bps);
        slippage_bps.append_value(r.slippage_bps);
        fixed_total_return.append_value(m.fixed_total_return);
        fixed_sharpe_ratio.append_value(m.fixed_sharpe_ratio);
        fixed_max_drawdown_pct.append_value(m.fixed_max_drawdown_pct);
        fixed_expectancy_bps.append_value(m.fixed_expectancy_bps);
        fixed_profit_factor.append_value(m.fixed_profit_factor);
        fill_close.append_value(u8::from(r.fill_close));
        buy_hold_pnl.append_value(r.buy_hold_pnl);
        buy_hold_return.append_value(r.buy_hold_return);
        bars_used.append_value(r.bars_used);
        run_status.append_value(r.status.as_str());
    }

    let cols: Vec<ArrayRef> = vec![
        Arc::new(strategy.finish()),
        Arc::new(symbol.finish()),
        Arc::new(timeframe.finish()),
        Arc::new(strategy_params.finish()),
        Arc::new(strategy_params_json.finish()),
        Arc::new(bal.finish()),
        Arc::new(trade_size.finish()),
        Arc::new(pnl.finish()),
        Arc::new(eq.finish()),
        Arc::new(mdd.finish()),
        Arc::new(trades.finish()),
        Arc::new(sharpe.finish()),
        Arc::new(sortino.finish()),
        Arc::new(wr.finish()),
        Arc::new(pf.finish()),
        Arc::new(calmar.finish()),
        Arc::new(return_mdd.finish()),
        Arc::new(avg_win.finish()),
        Arc::new(avg_loss.finish()),
        Arc::new(expectancy.finish()),
        Arc::new(consecutive_wins.finish()),
        Arc::new(consecutive_losses.finish()),
        Arc::new(volatility.finish()),
        Arc::new(recovery_factor.finish()),
        Arc::new(avg_hold_bars.finish()),
        Arc::new(avg_mae.finish()),
        Arc::new(avg_mfe.finish()),
        Arc::new(edge_ratio.finish()),
        Arc::new(wl_ratio.finish()),
        Arc::new(skewness.finish()),
        Arc::new(kurtosis.finish()),
        Arc::new(sqn.finish()),
        Arc::new(pf_stability.finish()),
        Arc::new(z_score.finish()),
        Arc::new(p_value.finish()),
        Arc::new(fixed_pnl.finish()),
        Arc::new(params_full.finish()),
        Arc::new(turnover_per_year.finish()),
        Arc::new(delay_bars.finish()),
        Arc::new(fee_bps.finish()),
        Arc::new(slippage_bps.finish()),
        Arc::new(fixed_total_return.finish()),
        Arc::new(fixed_sharpe_ratio.finish()),
        Arc::new(fixed_max_drawdown_pct.finish()),
        Arc::new(fixed_expectancy_bps.finish()),
        Arc::new(fixed_profit_factor.finish()),
        Arc::new(fill_close.finish()),
        Arc::new(buy_hold_pnl.finish()),
        Arc::new(buy_hold_return.finish()),
        Arc::new(bars_used.finish()),
        Arc::new(run_status.finish()),
    ];

    let fields = vec![
        Field::new("strategy", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("timeframe", DataType::Utf8, false),
        Field::new("strategy_params", DataType::UInt64, false),
        Field::new("strategy_params_json", DataType::Utf8, false),
        Field::new("balance", DataType::Float64, false),
        Field::new("trade_size", DataType::Utf8, false),
        Field::new("pnl", DataType::Float64, false),
        Field::new("eq", DataType::Float64, false),
        Field::new("mdd", DataType::Float64, false),
        Field::new("trades", DataType::UInt32, false),
        Field::new("sharpe", DataType::Float64, false),
        Field::new("sortino", DataType::Float64, false),
        Field::new("wr", DataType::Float64, false),
        Field::new("pf", DataType::Float64, false),
        Field::new("calmar", DataType::Float64, false),
        Field::new("return_mdd", DataType::Float64, false),
        Field::new("avg_win", DataType::Float64, false),
        Field::new("avg_loss", DataType::Float64, false),
        Field::new("expectancy", DataType::Float64, false),
        Field::new("consecutive_wins", DataType::UInt32, false),
        Field::new("consecutive_losses", DataType::UInt32, false),
        Field::new("volatility", DataType::Float64, false),
        Field::new("recovery_factor", DataType::Float64, false),
        Field::new("avg_hold_bars", DataType::Float64, false),
        Field::new("avg_mae", DataType::Float64, false),
        Field::new("avg_mfe", DataType::Float64, false),
        Field::new("edge_ratio", DataType::Float64, false),
        Field::new("wl_ratio", DataType::Float64, false),
        Field::new("skewness", DataType::Float64, false),
        Field::new("kurtosis", DataType::Float64, false),
        Field::new("sqn", DataType::Float64, false),
        Field::new("pf_stability", DataType::Float64, false),
        Field::new("z_score", DataType::Float64, false),
        Field::new("p_value", DataType::Float64, false),
        Field::new("fixed_pnl", DataType::Float64, false),
        Field::new("params", DataType::Utf8, false),
        Field::new("turnover_per_year", DataType::Float64, false),
        Field::new("delay_bars", DataType::UInt32, false),
        Field::new("fee_bps", DataType::Float64, false),
        Field::new("slippage_bps", DataType::Float64, false),
        Field::new("fixed_total_return", DataType::Float64, false),
        Field::new("fixed_sharpe_ratio", DataType::Float64, false),
        Field::new("fixed_max_drawdown_pct", DataType::Float64, false),
        Field::new("fixed_expectancy_bps", DataType::Float64, false),
        Field::new("fixed_profit_factor", DataType::Float64, false),
        Field::new("fill_close", DataType::UInt8, false),
        Field::new("buy_hold_pnl", DataType::Float64, false),
        Field::new("buy_hold_return", DataType::Float64, false),
        // 退化结果标记: 见 RunStatus。bars_used 与 status 必须配对解读。
        Field::new("bars_used", DataType::UInt32, false),
        Field::new("status", DataType::Utf8, false),
    ];

    let schema = arrow::datatypes::Schema::new(fields);
    RecordBatch::try_new(Arc::new(schema), cols).expect("Parquet schema/column count mismatch")
}

pub fn write_parquet(path: &str, results: &[RunResult], balance: f64) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let batch = build_parquet(results, balance);
    let schema = batch.schema();

    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_detail_parquet(
    details: &[crate::app::DetailResult],
    dir: &str,
    write_curves: bool,
) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::collections::HashMap;
    use std::fs::File;

    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path)?;

    let mut groups: HashMap<(String, String, String, String), Vec<&crate::app::DetailResult>> =
        HashMap::new();
    for d in details {
        groups
            .entry((
                d.symbol.clone(),
                d.timeframe.clone(),
                d.strategy.clone(),
                d.params.clone(),
            ))
            .or_default()
            .push(d);
    }

    for ((symbol, tf, strategy, params), group) in &groups {
        let clean_params = params.replace(' ', "_").replace('=', "-").replace(',', "_");
        let filename = format!("{}_{}_{}_{}.parquet", symbol, tf, strategy, clean_params);
        let filepath = path.join(filename);

        let mut trade_entry_time = Int64Builder::new();
        let mut trade_exit_time = Int64Builder::new();
        let mut trade_side = StringBuilder::new();
        let mut trade_entry = Float64Builder::new();
        let mut trade_exit = Float64Builder::new();
        let mut trade_qty = Float64Builder::new();
        let mut trade_pnl = Float64Builder::new();
        let mut trade_pnl_pct = Float64Builder::new();
        let mut trade_fixed_pnl = Float64Builder::new();
        let mut trade_bars_held = Int64Builder::new();
        let mut trade_max_runup = Float64Builder::new();
        let mut trade_max_drawdown = Float64Builder::new();
        let mut trade_exit_reason = StringBuilder::new();

        for d in group {
            for t in &d.trades {
                trade_entry_time.append_value(t.entry_time);
                trade_exit_time.append_value(t.exit_time);
                trade_side.append_value(if t.side == 1 { "LONG" } else { "SHORT" });
                trade_entry.append_value(t.entry_price);
                trade_exit.append_value(t.exit_price);
                trade_qty.append_value(t.qty);
                trade_pnl.append_value(t.pnl);
                trade_pnl_pct.append_value(t.pnl_pct);
                trade_fixed_pnl.append_value(t.fixed_pnl);
                trade_bars_held.append_value(t.bars_held);
                trade_max_runup.append_value(t.max_runup);
                trade_max_drawdown.append_value(t.max_drawdown);
                trade_exit_reason.append_value(&t.exit_reason);
            }
        }

        let trade_cols: Vec<ArrayRef> = vec![
            Arc::new(trade_entry_time.finish()),
            Arc::new(trade_exit_time.finish()),
            Arc::new(trade_side.finish()),
            Arc::new(trade_entry.finish()),
            Arc::new(trade_exit.finish()),
            Arc::new(trade_qty.finish()),
            Arc::new(trade_pnl.finish()),
            Arc::new(trade_pnl_pct.finish()),
            Arc::new(trade_fixed_pnl.finish()),
            Arc::new(trade_bars_held.finish()),
            Arc::new(trade_max_runup.finish()),
            Arc::new(trade_max_drawdown.finish()),
            Arc::new(trade_exit_reason.finish()),
        ];

        let trade_fields = vec![
            Field::new("entry_time", DataType::Int64, false),
            Field::new("exit_time", DataType::Int64, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("entry_price", DataType::Float64, false),
            Field::new("exit_price", DataType::Float64, false),
            Field::new("qty", DataType::Float64, false),
            Field::new("pnl", DataType::Float64, false),
            Field::new("pnl_pct", DataType::Float64, false),
            Field::new("fixed_pnl", DataType::Float64, false),
            Field::new("bars_held", DataType::Int64, false),
            Field::new("max_runup", DataType::Float64, false),
            Field::new("max_drawdown", DataType::Float64, false),
            Field::new("exit_reason", DataType::Utf8, false),
        ];

        let trade_schema = arrow::datatypes::Schema::new(trade_fields);
        let trade_batch = RecordBatch::try_new(Arc::new(trade_schema), trade_cols)?;

        if let Some(parent) = filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(&filepath)?;
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, trade_batch.schema(), Some(props))?;
        writer.write(&trade_batch)?;
        writer.close()?;

        // 分组元数据进 schema metadata, 文件名含 params 明文(含 `_` 清洗)故不
        // 做反向解析。两条曲线的 stem 相同, 只有 kind 后缀不同。
        let curve_meta = arrow::datatypes::Metadata::new()
            .with("symbol", symbol.clone())
            .with("timeframe", tf.clone())
            .with("strategy", strategy.clone())
            .with("params", params.clone());
        let stem = format!("{}_{}_{}_{}", symbol, tf, strategy, clean_params);

        // 复利腿逐 bar 权益。README 承诺 `detail/` 导出"逐笔交易/权益曲线",
        // 但历史上**只写了 trades**: 权益曲线从未落盘, 复算需重跑整轮回测。
        let mut bar_time = Int64Builder::new();
        let mut equity = Float64Builder::new();
        for d in group {
            for &(ts, eq) in &d.equity_curve {
                bar_time.append_value(ts as i64);
                equity.append_value(eq);
            }
        }
        write_curve_file(
            path,
            &stem,
            "equitycurve",
            &curve_meta,
            Field::new("time", DataType::Int64, false),
            Arc::new(bar_time.finish()),
            Field::new("equity", DataType::Float64, false),
            Arc::new(equity.finish()),
        )?;

        // fixed 腿逐 bar 权益: `--pbo` 的 T×N 输入。
        // `write_curves=false`（默认）时跳过: GB 级 I/O 只在做 PBO 时花。
        if !write_curves {
            continue;
        }
        let mut bar_idx = UInt64Builder::new();
        let mut fixed_equity = Float64Builder::new();
        for d in group {
            for (i, v) in d.fixed_curve.iter().enumerate() {
                bar_idx.append_value(i as u64);
                fixed_equity.append_value(*v);
            }
        }
        write_curve_file(
            path,
            &stem,
            "fixedcurve",
            &curve_meta,
            Field::new("bar_idx", DataType::UInt64, false),
            Arc::new(bar_idx.finish()),
            Field::new("fixed_equity", DataType::Float64, false),
            Arc::new(fixed_equity.finish()),
        )?;
    }

    Ok(())
}

/// 写一条逐 bar 曲线到 `{dir}/{stem}_{kind}.parquet`。
///
/// `kind` 为 `equitycurve` / `fixedcurve`; 两条曲线的**列语义不同**, 故索引列与
/// 值列由调用方给出而不共用一套 schema(避免"列名对但含义错"的静默误读)。
#[allow(clippy::too_many_arguments)]
fn write_curve_file(
    dir: &std::path::Path,
    stem: &str,
    kind: &str,
    meta: &arrow::datatypes::Metadata,
    idx_field: Field,
    idx_col: ArrayRef,
    val_field: Field,
    val_col: ArrayRef,
) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let schema =
        arrow::datatypes::Schema::new_with_metadata(vec![idx_field, val_field], meta.clone());
    let batch = RecordBatch::try_new(Arc::new(schema), vec![idx_col, val_col])?;
    let file = File::create(dir.join(format!("{stem}_{kind}.parquet")))?;
    let mut writer = ArrowWriter::try_new(
        file,
        batch.schema(),
        Some(WriterProperties::builder().build()),
    )?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// 按**列名**取列序号; 缺列时 panic 并列出实际列名。
///
/// 优于魔法列号: 曲线文件的列顺序由 schema 决定, 用常量下标读取会在 schema
/// 调整后**静默读错列**(类型恰好兼容时不报错, 数值静默错位)。
pub fn col_by_name(
    schema: &arrow::datatypes::Schema,
    name: &str,
    file: &std::path::Path,
) -> Result<usize> {
    schema.index_of(name).map_err(|_| {
        crate::error::AppError::Msg(format!(
            "{} 缺列 {name:?}; 实际列: {:?}",
            file.display(),
            schema
                .fields()
                .iter()
                .map(|f| f.name().as_str())
                .collect::<Vec<_>>()
        ))
    })
}

/// 权益曲线文件元组。
#[derive(Debug, Clone)]
pub struct EquityCurve {
    pub symbol: String,
    pub timeframe: String,
    pub strategy: String,
    pub params: String,
    /// bar 时间戳(毫秒)。
    pub times: Vec<i64>,
    /// 复利腿逐 bar 权益。
    pub values: Vec<f64>,
}

/// 读取目录下全部 `*_equitycurve.parquet`(一般为 detail/ 目录)。
///
/// 与 [`read_fixed_curves`] 同构但**列语义不同**: 前者返回 `(time, equity)`,
/// 后者返回 `(bar_idx, fixed_equity)`。刻意不复用同一个 reader —— 两条曲线的
/// 索引列类型(Int64 vs UInt64)、含义(时间戳 vs 序号)都不同, 混用会静默误读。
pub fn read_equity_curves(dir: &str) -> Result<Vec<EquityCurve>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs;
    use std::fs::File;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        crate::error::AppError::Msg(format!("读取 equitycurve 目录失败 {dir}: {e}"))
    })?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "parquet")
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().ends_with("_equitycurve"))
        })
        .collect();
    paths.sort();
    for path in paths {
        let file = File::open(&path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let schema = builder.schema().clone();
        let idx = col_by_name(&schema, "time", &path)?;
        let val = col_by_name(&schema, "equity", &path)?;
        let meta = schema.metadata().clone();
        let get = |k: &str| -> Result<String> {
            meta.get(k).cloned().ok_or_else(|| {
                crate::error::AppError::Msg(format!(
                    "equitycurve 缺分组元数据 {k}: {}",
                    path.display()
                ))
            })
        };
        let symbol = get("symbol")?;
        let timeframe = get("timeframe")?;
        let strategy = get("strategy")?;
        let params = get("params")?;
        let reader = builder.build()?;
        let mut times = Vec::new();
        let mut values = Vec::new();
        for batch in reader {
            let batch = batch?;
            times.extend(i64_col(&batch, idx));
            values.extend(f64_col(&batch, val));
        }
        out.push(EquityCurve {
            symbol,
            timeframe,
            strategy,
            params,
            times,
            values,
        });
    }
    Ok(out)
}

/// fixed 曲线文件元组(供 `--pbo` 按(币,周期,策略)分组)。
pub struct FixedCurve {
    pub symbol: String,
    pub timeframe: String,
    pub strategy: String,
    pub params: String,
    pub values: Vec<f64>,
}

/// 读取目录下全部 `*_fixedcurve.parquet`(一般为 detail/ 目录)。
pub fn read_fixed_curves(dir: &str) -> Result<Vec<FixedCurve>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs;
    use std::fs::File;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir)
        .map_err(|e| crate::error::AppError::Msg(format!("读取 fixedcurve 目录失败 {dir}: {e}")))?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "parquet")
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().ends_with("_fixedcurve"))
        })
        .collect();
    paths.sort();
    for path in paths {
        let file = File::open(&path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let meta = builder.schema().metadata().clone();
        let get = |k: &str| -> Result<String> {
            meta.get(k).cloned().ok_or_else(|| {
                crate::error::AppError::Msg(format!(
                    "fixedcurve 缺分组元数据 {k}: {}",
                    path.display()
                ))
            })
        };
        let symbol = get("symbol")?;
        let timeframe = get("timeframe")?;
        let strategy = get("strategy")?;
        let params = get("params")?;
        let reader = builder.build()?;
        let mut values = Vec::new();
        for batch in reader {
            let batch = batch?;
            values.extend(f64_col(&batch, 1));
        }
        out.push(FixedCurve {
            symbol,
            timeframe,
            strategy,
            params,
            values,
        });
    }
    Ok(out)
}

fn str_col(batch: &RecordBatch, idx: usize) -> Vec<String> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("string column");
    (0..batch.num_rows())
        .map(|i| arr.value(i).to_string())
        .collect()
}

fn i64_col(batch: &RecordBatch, idx: usize) -> Vec<i64> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("i64 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

fn u64_col(batch: &RecordBatch, idx: usize) -> Vec<u64> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .expect("u64 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

fn f64_col(batch: &RecordBatch, idx: usize) -> Vec<f64> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("f64 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

fn u32_col(batch: &RecordBatch, idx: usize) -> Vec<u32> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("u32 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

fn u8_col(batch: &RecordBatch, idx: usize) -> Vec<u8> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt8Array>()
        .expect("u8 column");
    (0..batch.num_rows()).map(|i| arr.value(i)).collect()
}

/// 按**列名**索引 parquet 列。取代原先 40 个魔法列号。
///
/// 为什么必须按名: `read_parquet` 既读**本次写的**文件也读**历史版本写的**缓存。
/// 用常量下标读取时, 只要列的顺序或数量变化(每追加一列就是一次), 下标就可能
/// 落到类型恰好兼容的另一列上 —— 不报错, 数值**静默错位**(例如把 `sharpe`
/// 读成 `sortino`, 两者都是 `f64`)。按名读取 + 缺列时列出实际列名, 让错配在
/// 第一次发生时就炸掉。
///
/// - [`ColumnIndex::req`] 必须存在, 缺则 `Err` 并列出实际列名。
/// - [`ColumnIndex::opt`] 可选(旧缓存文件没有该列), 由调用方决定回退值。
#[derive(Debug, Clone)]
struct ColumnIndex {
    map: std::collections::HashMap<String, usize>,
    file: String,
}

impl ColumnIndex {
    fn new(schema: &arrow::datatypes::Schema, file: &str) -> Self {
        Self {
            map: schema
                .fields()
                .iter()
                .enumerate()
                .map(|(i, f)| (f.name().clone(), i))
                .collect(),
            file: file.to_string(),
        }
    }

    fn req(&self, name: &str) -> Result<usize> {
        self.map.get(name).copied().ok_or_else(|| {
            let mut actual: Vec<&str> = self.map.keys().map(String::as_str).collect();
            actual.sort_unstable();
            crate::error::AppError::Msg(format!(
                "{} 缺必需列 {name:?}; 实际列: {actual:?}(旧格式缓存请删 temps/ 后重跑)",
                self.file
            ))
        })
    }

    fn opt(&self, name: &str) -> Option<usize> {
        self.map.get(name).copied()
    }
}

/// 读取一个组合临时文件/报告文件。
///
/// 兼容策略: `turnover_per_year` 及其后的列都是**历史上逐次追加**的, 旧缓存文件
/// 没有它们。原先用 `batch.num_columns() > 37` 这类**列数**判断, 一旦有人调整列
/// 顺序就会把"有但不同名"的列误判成"没有"。现在统一走 [`ColumnIndex::opt`]:
/// 按名字找, 找不到才回退。
pub fn read_parquet(path: &str) -> Result<Vec<RunResult>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let ci = ColumnIndex::new(&schema, path);
    let reader = builder.build()?;

    // 历史行为: 缺 params 列直接报错(旧格式无法区分 ssf/ssf_st 等 variant)。
    let params_idx = ci.req("params")?;
    let strategy_idx = ci.req("strategy")?;
    let symbol_idx = ci.req("symbol")?;
    let timeframe_idx = ci.req("timeframe")?;
    let pnl_idx = ci.req("pnl")?;
    let eq_idx = ci.req("eq")?;
    let mdd_idx = ci.req("mdd")?;
    let trades_idx = ci.req("trades")?;
    let sharpe_idx = ci.req("sharpe")?;
    let sortino_idx = ci.req("sortino")?;
    let wr_idx = ci.req("wr")?;
    let pf_idx = ci.req("pf")?;
    let calmar_idx = ci.req("calmar")?;
    let avg_win_idx = ci.req("avg_win")?;
    let avg_loss_idx = ci.req("avg_loss")?;
    let expectancy_idx = ci.req("expectancy")?;
    let cw_idx = ci.req("consecutive_wins")?;
    let cl_idx = ci.req("consecutive_losses")?;
    let volatility_idx = ci.req("volatility")?;
    let recovery_idx = ci.req("recovery_factor")?;
    let avg_hold_idx = ci.req("avg_hold_bars")?;
    let avg_mae_idx = ci.req("avg_mae")?;
    let avg_mfe_idx = ci.req("avg_mfe")?;
    let edge_idx = ci.req("edge_ratio")?;
    let wl_idx = ci.req("wl_ratio")?;
    let skew_idx = ci.req("skewness")?;
    let kurt_idx = ci.req("kurtosis")?;
    let sqn_idx = ci.req("sqn")?;
    let pf_stab_idx = ci.req("pf_stability")?;
    let z_idx = ci.req("z_score")?;
    let pval_idx = ci.req("p_value")?;
    let fixed_pnl_idx = ci.req("fixed_pnl")?;

    let mut all = Vec::new();
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows();

        let strategy = str_col(&batch, strategy_idx);
        let symbol = str_col(&batch, symbol_idx);
        let timeframe = str_col(&batch, timeframe_idx);
        let params_full = str_col(&batch, params_idx);
        let pnl = f64_col(&batch, pnl_idx);
        let eq = f64_col(&batch, eq_idx);
        let mdd = f64_col(&batch, mdd_idx);
        let trades = u32_col(&batch, trades_idx);
        let sharpe = f64_col(&batch, sharpe_idx);
        let sortino = f64_col(&batch, sortino_idx);
        let wr = f64_col(&batch, wr_idx);
        let pf = f64_col(&batch, pf_idx);
        let calmar = f64_col(&batch, calmar_idx);
        let avg_win = f64_col(&batch, avg_win_idx);
        let avg_loss = f64_col(&batch, avg_loss_idx);
        let expectancy = f64_col(&batch, expectancy_idx);
        let cw = u32_col(&batch, cw_idx);
        let cl = u32_col(&batch, cl_idx);
        let volatility = f64_col(&batch, volatility_idx);
        let recovery = f64_col(&batch, recovery_idx);
        let avg_hold = f64_col(&batch, avg_hold_idx);
        let avg_mae = f64_col(&batch, avg_mae_idx);
        let avg_mfe = f64_col(&batch, avg_mfe_idx);
        let edge = f64_col(&batch, edge_idx);
        let wl = f64_col(&batch, wl_idx);
        let skew = f64_col(&batch, skew_idx);
        let kurt = f64_col(&batch, kurt_idx);
        let sqn = f64_col(&batch, sqn_idx);
        let pf_stab = f64_col(&batch, pf_stab_idx);
        let z_score = f64_col(&batch, z_idx);
        let p_value = f64_col(&batch, pval_idx);
        let fixed_pnl = f64_col(&batch, fixed_pnl_idx);

        // 以下均为**历史上逐次追加**的可选列, 旧缓存文件没有 → 回退为旧口径。
        let turnover_per_year = opt_f64(&batch, &ci, "turnover_per_year", n);
        let delay_bars = opt_u32(&batch, &ci, "delay_bars", n);
        let fee_bps = opt_f64(&batch, &ci, "fee_bps", n);
        let slippage_bps = opt_f64(&batch, &ci, "slippage_bps", n);
        let fixed_total_return = opt_f64(&batch, &ci, "fixed_total_return", n);
        let fixed_sharpe_ratio = opt_f64(&batch, &ci, "fixed_sharpe_ratio", n);
        let fixed_max_drawdown_pct = opt_f64(&batch, &ci, "fixed_max_drawdown_pct", n);
        let fixed_expectancy_bps = opt_f64(&batch, &ci, "fixed_expectancy_bps", n);
        let fixed_profit_factor = opt_f64(&batch, &ci, "fixed_profit_factor", n);
        let fill_close = opt_u8(&batch, &ci, "fill_close", n);
        let buy_hold_pnl = opt_f64(&batch, &ci, "buy_hold_pnl", n);
        let buy_hold_return = opt_f64(&batch, &ci, "buy_hold_return", n);
        // O6: 原样读回写入时的编码, 合并时不重算。旧文件无此列 → 0 / 空串。
        let strategy_params = opt_u64(&batch, &ci, "strategy_params", n);
        let strategy_params_json = ci
            .opt("strategy_params_json")
            .map(|i| str_col(&batch, i))
            .unwrap_or_else(|| vec![String::new(); n]);
        // L5: 退化结果标记。旧文件无此列 → Ok / bars_used = 0(表示"未记录")。
        let bars_used = opt_u32(&batch, &ci, "bars_used", n);
        let status = ci
            .opt("status")
            .map(|i| str_col(&batch, i))
            .map(|v| {
                v.iter()
                    .map(|s| match s.as_str() {
                        "ok" => crate::app::RunStatus::Ok,
                        "no_signals" => crate::app::RunStatus::NoSignals,
                        "no_trades" => crate::app::RunStatus::NoTrades,
                        _ => crate::app::RunStatus::NoSignals,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![crate::app::RunStatus::default(); n]);

        for i in 0..n {
            all.push(RunResult {
                symbol: symbol[i].clone(),
                timeframe: timeframe[i].clone(),
                strategy: strategy[i].clone(),
                params: params_full[i].clone(),
                strategy_params: strategy_params[i],
                strategy_params_json: strategy_params_json[i].clone(),
                delay_bars: delay_bars[i],
                fee_bps: fee_bps[i],
                slippage_bps: slippage_bps[i],
                fill_close: fill_close[i] != 0,
                buy_hold_pnl: buy_hold_pnl[i],
                buy_hold_return: buy_hold_return[i],
                bars_used: bars_used[i],
                status: status[i],
                metrics: SummaryMetrics {
                    total_return_pct: pnl[i] * 100.0,
                    total_return: pnl[i],
                    sharpe_ratio: sharpe[i],
                    sortino_ratio: sortino[i],
                    max_drawdown_pct: mdd[i] * 100.0,
                    win_rate: wr[i] * 100.0,
                    profit_factor: pf[i],
                    calmar_ratio: calmar[i],
                    num_trades: trades[i],
                    avg_win: avg_win[i],
                    avg_loss: avg_loss[i],
                    expectancy: expectancy[i],
                    consecutive_wins: cw[i],
                    consecutive_losses: cl[i],
                    final_equity: eq[i],
                    volatility: volatility[i],
                    recovery_factor: recovery[i],
                    avg_hold_bars: avg_hold[i],
                    avg_mae: avg_mae[i],
                    avg_mfe: avg_mfe[i],
                    edge_ratio: edge[i],
                    wl_ratio: wl[i],
                    skewness: skew[i],
                    kurtosis: kurt[i],
                    sqn: sqn[i],
                    pf_stability: pf_stab[i],
                    z_score: z_score[i],
                    p_value: p_value[i],
                    total_fixed_pnl: fixed_pnl[i],
                    turnover_per_year: turnover_per_year[i],
                    delay_bars: delay_bars[i],
                    fee_bps: fee_bps[i],
                    slippage_bps: slippage_bps[i],
                    fixed_total_return: fixed_total_return[i],
                    fixed_sharpe_ratio: fixed_sharpe_ratio[i],
                    fixed_max_drawdown_pct: fixed_max_drawdown_pct[i],
                    fixed_expectancy_bps: fixed_expectancy_bps[i],
                    fixed_profit_factor: fixed_profit_factor[i],
                },
            });
        }
    }
    Ok(all)
}

/// 可选 `f64` 列; 旧缓存文件无此列时返回长度 `n` 的默认值。
fn opt_f64(batch: &RecordBatch, ci: &ColumnIndex, name: &str, n: usize) -> Vec<f64> {
    ci.opt(name)
        .map_or_else(|| vec![0.0; n], |i| f64_col(batch, i))
}

/// 可选 `u32` 列。
fn opt_u32(batch: &RecordBatch, ci: &ColumnIndex, name: &str, n: usize) -> Vec<u32> {
    ci.opt(name)
        .map_or_else(|| vec![0; n], |i| u32_col(batch, i))
}

/// 可选 `u64` 列。
fn opt_u64(batch: &RecordBatch, ci: &ColumnIndex, name: &str, n: usize) -> Vec<u64> {
    ci.opt(name)
        .map_or_else(|| vec![0; n], |i| u64_col(batch, i))
}

/// 可选 `u8` 列。
fn opt_u8(batch: &RecordBatch, ci: &ColumnIndex, name: &str, n: usize) -> Vec<u8> {
    ci.opt(name)
        .map_or_else(|| vec![0; n], |i| u8_col(batch, i))
}

pub fn write_single_parquet(
    results: &[RunResult],
    path: &std::path::Path,
    balance: f64,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_parquet(path.to_str().expect("non-UTF-8 path"), results, balance)
}

/// 合并本次运行产生的组合文件(含跳过的缓存文件), 输出为最终的 result.parquet。
/// 仅读取显式传入的路径, 避免聚合 `temps/` 中遗留的旧参数文件。
///
/// 合并若干组合临时文件为单一报告。
///
/// **读失败不再静默**: 原实现只 `eprintln!` 一行然后跳过, 返回值仍是成功,
/// 调用方无从得知丢了几个组合 —— 报告里少了参数却显示"完成"。
/// 现在返回 [`MergeStats`], 失败数进入最终摘要; 全部失败才报错。
pub fn merge_parquet_files(
    paths: &[PathBuf],
    output_path: &str,
    balance: f64,
) -> Result<MergeStats> {
    let mut all: Vec<RunResult> = Vec::new();
    let mut failed = 0u32;
    for path in paths {
        match read_parquet(path.to_str().expect("non-UTF-8 path")) {
            Ok(mut results) => all.append(&mut results),
            Err(e) => {
                eprintln!("  [警告] 读取失败 {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    if all.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "合并失败: {} 个组合文件全部无法读取, 未产出 {}",
            failed, output_path
        )));
    }
    let total = all.len() as u64;
    write_parquet(output_path, &all, balance)?;
    Ok(MergeStats {
        merged_rows: total,
        failed_files: failed,
        total_files: paths.len() as u32,
    })
}

/// [`merge_parquet_files`] 的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeStats {
    pub merged_rows: u64,
    /// 读失败被跳过的组合文件数。
    pub failed_files: u32,
    pub total_files: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_value_fast_slow_encoding() {
        assert_eq!(strategy_params_value("fast=10,slow=20"), 10_000_020);
        assert_eq!(strategy_params_value("fast=50,slow=200"), 50_000_200);
        // 旧口径回归。
        assert_eq!(strategy_params_value("period=20"), 20);
        assert_eq!(strategy_params_value("ema_10"), 10);
    }

    #[test]
    fn test_params_value_new_strategies_unique() {
        // 海龟/DT/RBreaker/ATR带/Keltner/BBMR/RSI2/Aberration 编码各策略内唯一。
        assert_eq!(strategy_params_value("entry=20,exit=10"), 20_000_010);
        assert_eq!(strategy_params_value("entry=55,exit=10"), 55_000_010);
        assert_eq!(strategy_params_value("n=5,k=0.2"), 5_000_200);
        assert_eq!(strategy_params_value("n=50,m=2"), 50_002_000);
        assert_eq!(strategy_params_value("stop=0.5"), 50);
        assert_eq!(strategy_params_value("stop=2"), 200);
        assert_eq!(strategy_params_value("k=1.5"), 1500);
        assert_eq!(strategy_params_value("sigma=2.5"), 2500);
        assert_eq!(strategy_params_value("buy=10,sell=90"), 1_000_900);
        assert_eq!(strategy_params_value("buy=5,sell=95"), 500_950);
        assert_eq!(strategy_params_value("sigma=2.5,adx=20"), 2_500_000_200);
        assert_eq!(strategy_params_value("sigma=2,adx=25"), 2_000_000_250);
        assert_eq!(strategy_params_value("sigma=2.5,exitn=10"), 2_500_000_100);
        assert_eq!(strategy_params_value("sigma=2.5,exitb=0.5"), 2_500_000_005);
        assert_eq!(strategy_params_value("sigma=2.5,exitb=0.8"), 2_500_000_008);
        assert_eq!(strategy_params_value("sigma=2.5,body=0.5"), 2_500_000_005);
        assert_eq!(strategy_params_value("sigma=2.5,smareg=100"), 2_500_001_000);
        assert_eq!(strategy_params_value("sigma=2.5,hmalen=100"), 2_500_001_000);
        assert_eq!(strategy_params_value("ma=hma,malen=100"), 7_000_100);
        // 该编码对应的人类可读 JSON 必须是合法值(见 test_params_json_is_always_valid_json)。
        assert_eq!(
            params_to_json("ma=hma,malen=100"),
            "{\"ma\":\"hma\",\"malen\":\"100\"}"
        );
        assert_eq!(strategy_params_value("ma=sma,malen=200"), 200);
        assert_eq!(strategy_params_value("ma=sma,len=20,sigma=2.5"), 202_500);
        assert_eq!(strategy_params_value("ma=hma,len=5,sigma=3"), 20_053_000);
        assert_eq!(strategy_params_value("sigma=2.5,rexn=7"), 2_500_000_070);
        assert_eq!(
            strategy_params_value("sigma=2.5,voln=10,volmult=1.5"),
            250_010_015
        );
        // 溢出回退（2026-10-12）：旧4键碰撞对必须分开，且VOL全网格720串唯一。
        assert_ne!(
            strategy_params_value("sigma=1.5,bblen=60,voln=5,volmult=2"),
            strategy_params_value("sigma=2,bblen=10,voln=5,volmult=2")
        );
        assert_eq!(strategy_params_value("sigma=2.5,atrthr=1"), 2_500_000_010);
        assert_eq!(
            strategy_params_value("sigma=2.5,kc=1.5,ttmn=5"),
            250_015_005
        );
        assert_eq!(strategy_params_value("sigma=3,kc=2.5,ttmn=10"), 300_025_010);
        assert_eq!(strategy_params_value("reset=0"), 0);
        assert_eq!(strategy_params_value("reset=1"), 1);
        assert_eq!(strategy_params_value("sigma=2.5,trailk=3"), 2_500_000_030);
        // 未知形态不再静默编码为 0 —— 那会与真实的 period=0 撞值, 使两个不同
        // 参数在 strategy_params 列上不可区分。现在退化为 FNV-32 并 warn!。
        let unknown = strategy_params_value("base");
        assert_ne!(unknown, 0, "未知形态不得退化为 0");
        assert_ne!(
            unknown,
            strategy_params_value("period=0"),
            "未知形态不得与真实的 period=0 撞值"
        );
        assert_eq!(
            unknown,
            u64::from(super::fnv1a_32("base")),
            "未知形态的回退必须是确定性的 FNV-32"
        );
    }

    #[test]
    fn test_params_value_vol_4key_grid_unique() {
        // VOL宽网格（σ6×len6×N4×m5=720）编码无碰撞。
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for s in ["1.5", "2", "2.5", "3", "3.5", "4"] {
            for l in [10, 20, 30, 40, 50, 60] {
                for n in [5, 10, 15, 20] {
                    for m in ["1.2", "1.5", "2", "2.5", "3"] {
                        let p = format!("sigma={s},bblen={l},voln={n},volmult={m}");
                        assert!(seen.insert(strategy_params_value(&p)), "collision: {p}");
                    }
                }
            }
        }
        assert_eq!(seen.len(), 720);
    }

    #[test]
    fn test_fixed_curves_gated_by_flag() {
        // fixed_curves=false 只落逐笔 trades，不写 *_fixedcurve.parquet。
        let d = crate::app::DetailResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            strategy: "BBRK".into(),
            params: "sigma=2.5".into(),
            trades: vec![],
            equity_curve: vec![],
            fixed_curve: vec![1.0, 2.0, 3.0],
        };
        let dir = std::env::temp_dir().join("vta_test_nocurves");
        let _ = std::fs::remove_dir_all(&dir);
        write_detail_parquet(std::slice::from_ref(&d), dir.to_str().unwrap(), false).unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|f| !f.contains("fixedcurve")), "{names:?}");
        assert!(!names.iter().any(|f| f.contains("fixedcurve")), "{names:?}");
        let dir2 = std::env::temp_dir().join("vta_test_curves");
        let _ = std::fs::remove_dir_all(&dir2);
        write_detail_parquet(std::slice::from_ref(&d), dir2.to_str().unwrap(), true).unwrap();
        let names2: Vec<String> = std::fs::read_dir(&dir2)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names2.iter().any(|f| f.contains("fixedcurve")),
            "{names2:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// `strategy_params_json` 整列必须是**合法 JSON**: 值一律为字符串并转义。
    /// 历史缺陷: 数字形态内插为裸数字、字符串形态内插为裸 token(`{"ma":hma}`),
    /// 两者与同列内的合法值并存, 使整列无法被 JSON 工具消费。
    #[test]
    fn test_params_json_is_always_valid_json() {
        for p in [
            "entry=20,exit=10",
            "n=5,k=0.2",
            "sigma=2.5",
            "buy=10,sell=90",
            "ma=hma,malen=100",
            "period=20,variant=Three",
            "fast=10,slow=30",
            "ema_100",
            "ma=hma",
        ] {
            let js = params_to_json(p);
            // 合法 JSON 对象: 以 { 开头、以 } 结尾、内部引号成对
            assert!(js.starts_with('{') && js.ends_with('}'), "{p} → {js}");
            assert_eq!(js.matches('"').count() % 2, 0, "{p} → {js} 引号不成对");
            assert!(!js.contains(": hma"), "{p} → {js} 含未加引号的 token");
            // 无未转义的裸引号/反斜杠
            assert!(!js.contains(char::from(92u8)), "{p} → {js} 含裸反斜杠");
        }
        // 具体形态: 值统一为字符串
        assert_eq!(
            params_to_json("entry=20,exit=10"),
            "{\"entry\":\"20\",\"exit\":\"10\"}"
        );
        assert_eq!(params_to_json("n=5,k=0.2"), "{\"n\":\"5\",\"k\":\"0.2\"}");
        assert_eq!(params_to_json("sigma=2.5"), "{\"sigma\":\"2.5\"}");
        assert_eq!(
            params_to_json("ma=hma,malen=100"),
            "{\"ma\":\"hma\",\"malen\":\"100\"}"
        );
        assert_eq!(
            params_to_json("period=20,variant=Three"),
            "{\"period\":\"20\",\"variant\":\"Three\"}"
        );
        assert_eq!(params_to_json("ema_100"), "{\"period\":\"100\"}");
    }

    /// 转义: 键值内的引号/反斜杠/控制字符必须被转义, 否则 JSON 失效。
    #[test]
    fn test_json_str_escapes_control_characters() {
        assert_eq!(json_str("abc"), "\"abc\"");
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_str("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_str("a\nb"), "\"a\\nb\"");
        assert_eq!(json_str("a\tb"), "\"a\\tb\"");
        assert_eq!(json_str("a\u{1}b"), "\"a\\u0001b\"");
    }

    /// README 承诺 `detail/` 导出"逐笔交易/**权益曲线**", 但历史上只写了 trades
    /// —— 权益曲线从未落盘, 复算必须重跑整轮回测。detail 必须**无条件**产出它
    /// (不像 fixedcurve 只在 PBO 时写)。
    #[test]
    fn test_detail_writes_equity_curve_regardless_of_flag() {
        use crate::app::DetailResult;

        let dir = std::env::temp_dir().join(format!("vta-eqcurve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mk = |param: &str, off: f64| DetailResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: param.into(),
            trades: Vec::new(),
            equity_curve: (0..5)
                .map(|i| {
                    (
                        1_700_000_000.0 + i as f64 * 86_400.0 + off,
                        1000.0 + off + i as f64,
                    )
                })
                .collect(),
            fixed_curve: (0..5).map(|i| 1000.0 + off + i as f64).collect(),
        };
        // write_curves = false: fixedcurve 跳过, equitycurve 仍必须落盘
        let details = vec![mk("period=10", 0.0), mk("period=20", 500.0)];
        write_detail_parquet(&details, dir.to_str().unwrap(), false).unwrap();

        let files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            files
                .iter()
                .filter(|f| f.ends_with("_equitycurve.parquet"))
                .count(),
            2,
            "每个参数组都应有 equitycurve: {files:?}"
        );
        assert!(
            !files.iter().any(|f| f.ends_with("_fixedcurve.parquet")),
            "write_curves=false 不应写 fixedcurve: {files:?}"
        );

        // 内容: time 与 equity 两列, 组内多参数按顺序拼接
        let p = dir.join("BTCUSDT_1d_sma_period-10_equitycurve.parquet");
        let curves = read_equity_curves(dir.to_str().unwrap()).unwrap();
        let c = curves
            .iter()
            .find(|c| c.params == "period=10")
            .expect("应能按 metadata 读回分组");
        assert_eq!(c.symbol, "BTCUSDT");
        assert_eq!(c.timeframe, "1d");
        assert_eq!(c.strategy, "sma");
        assert_eq!(c.times.len(), 5);
        assert_eq!(c.values.len(), 5);
        assert_eq!(c.times[0], 1_700_000_000);
        assert_eq!(c.values[4], 1004.0);
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `write_curves = true` 时两条曲线都要有, 且列名不同(语义不同)。
    #[test]
    fn test_fixed_and_equity_curves_have_distinct_columns() {
        use crate::app::DetailResult;

        let dir = std::env::temp_dir().join(format!("vta-curves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let details = vec![DetailResult {
            symbol: "ETHUSDT".into(),
            timeframe: "1h".into(),
            strategy: "ema".into(),
            params: "period=9".into(),
            trades: Vec::new(),
            equity_curve: (0..3)
                .map(|i| (1_700_000_000.0 + i as f64 * 3600.0, 500.0 + i as f64))
                .collect(),
            fixed_curve: vec![500.0, 501.0, 502.0],
        }];
        write_detail_parquet(&details, dir.to_str().unwrap(), true).unwrap();

        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        for (name, expect) in [
            ("_equitycurve.parquet", vec!["time", "equity"]),
            ("_fixedcurve.parquet", vec!["bar_idx", "fixed_equity"]),
        ] {
            let p = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.to_string_lossy().ends_with(name))
                .unwrap_or_else(|| panic!("缺少 {name}"));
            let f = std::fs::File::open(&p).unwrap();
            let b = ParquetRecordBatchReaderBuilder::try_new(f).unwrap();
            let cols: Vec<String> = b
                .schema()
                .fields()
                .iter()
                .map(|fld| fld.name().clone())
                .collect();
            assert_eq!(cols, expect, "{name} 列名/语义不符");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L5: 退化结果必须在报告里**可被筛掉**, 且 `bars_used` 必须落盘。
    ///
    /// 反例: 892 个参数中 190 个(21.3%)返回全 Hold, 指标全 0。若无 status 列,
    /// 它们与"零波动低风险"的真实结论在数值上不可区分, 取 top-N 时会挤掉真实结果。
    #[test]
    fn test_report_carries_bars_used_and_status() {
        let mk = |params: &str, bars: u32, status: crate::app::RunStatus| RunResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: params.into(),
            strategy_params: strategy_params_value(params),
            strategy_params_json: params_to_json(params),
            metrics: SummaryMetrics::default(),
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: 0.0,
            buy_hold_return: 0.0,
            bars_used: bars,
            status,
        };
        let rows = vec![
            mk("period=20", 300, crate::app::RunStatus::Ok),
            mk("period=400", 300, crate::app::RunStatus::NoSignals),
            mk("period=20", 20_000, crate::app::RunStatus::NoTrades),
        ];
        let dir = std::env::temp_dir().join(format!("vta-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");
        write_parquet(p.to_str().unwrap(), &rows, 10_000.0).unwrap();

        let back = read_parquet(p.to_str().unwrap()).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].bars_used, 300);
        assert_eq!(back[0].status, crate::app::RunStatus::Ok);
        assert_eq!(back[1].bars_used, 300);
        assert_eq!(back[1].status, crate::app::RunStatus::NoSignals);
        // 同一 params、不同数据规模 -> status/bars_used 是唯一区分依据
        assert_eq!(back[0].params, back[2].params);
        assert_ne!(back[0].bars_used, back[2].bars_used);
        assert_ne!(back[0].status, back[2].status);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O6: `strategy_params` / `strategy_params_json` 必须**原样往返**。
    ///
    /// 原实现在合并时从 `params` 重新推导两个列。若编码规则与写盘时不一致
    /// (或 params 串在两套版本间变过), 合并后的报告数值会被悄悄改写。
    #[test]
    fn test_params_encoding_survives_merge_unchanged() {
        let r = RunResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            // 刻意给一个"不可能由 params 推导出来"的值
            strategy_params: 999_999_999_999,
            strategy_params_json: "{\"sentinel\":\"1\"}".into(),
            metrics: SummaryMetrics::default(),
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: 0.0,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let dir = std::env::temp_dir().join(format!("vta-o6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.parquet");
        let out = dir.join("m.parquet");
        write_parquet(a.to_str().unwrap(), std::slice::from_ref(&r), 10_000.0).unwrap();
        merge_parquet_files(&[a], out.to_str().unwrap(), 10_000.0).unwrap();
        let back = read_parquet(out.to_str().unwrap()).unwrap();
        assert_eq!(
            back[0].strategy_params, 999_999_999_999,
            "strategy_params 被重算了"
        );
        assert_eq!(
            back[0].strategy_params_json, "{\"sentinel\":\"1\"}",
            "strategy_params_json 被重算了"
        );
        // strategy_params 必须是 UInt64: UInt32 会在大编码上静默回绕
        let v = back[0].strategy_params;
        assert!(
            v > u64::from(u32::MAX),
            "该值放不进 UInt32, 列类型必须已扩宽"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O5: `read_parquet` 按**列名**定位。缺必需列时必须报错并列出实际列名,
    /// 而不是把列号落到另一列上。
    ///
    /// 反例: 用魔法列号时, 删掉 `sharpe` 列会让 `sortino`(同为 `f64`)被当作
    /// `sharpe` 读进来 —— 不报错, 数值静默错位。
    #[test]
    fn read_reports_missing_required_column_with_actual_names() {
        use parquet::arrow::ArrowWriter;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        use parquet::file::properties::WriterProperties;

        let dir = std::env::temp_dir().join(format!("vta-colidx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1) 先写一个正常的单行文件
        let r = RunResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            strategy_params: 20,
            strategy_params_json: "{\"period\":\"20\"}".into(),
            metrics: SummaryMetrics {
                sharpe_ratio: 1.5,
                sortino_ratio: 2.5,
                ..SummaryMetrics::default()
            },
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: 0.0,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let full = dir.join("full.parquet");
        write_parquet(full.to_str().unwrap(), std::slice::from_ref(&r), 10_000.0).unwrap();

        // 2) 原样读回, 再造一个**只缺 sharpe** 的文件
        let src = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(&full).unwrap())
            .unwrap()
            .build()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let keep: Vec<usize> = src
            .schema()
            .fields()
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name() != "sharpe")
            .map(|(i, _)| i)
            .collect();
        let new_fields: Vec<Field> = keep
            .iter()
            .map(|&i| src.schema().field(i).clone())
            .collect();
        let new_schema = arrow::datatypes::Schema::new(new_fields);
        let cols: Vec<ArrayRef> = keep.iter().map(|&i| src.column(i).clone()).collect();
        let batch = RecordBatch::try_new(Arc::new(new_schema), cols).unwrap();
        let cut = dir.join("cut.parquet");
        let mut w = ArrowWriter::try_new(
            std::fs::File::create(&cut).unwrap(),
            batch.schema(),
            Some(WriterProperties::builder().build()),
        )
        .unwrap();
        w.write(&batch).unwrap();
        w.close().unwrap();

        // 3) 必须点名 sharpe 缺失; 绝不能把 sortino 当 sharpe 读进来
        let err = read_parquet(cut.to_str().unwrap())
            .expect_err("缺 sharpe 列必须报错, 不能把 sortino 当 sharpe 读进来");
        let msg = format!("{err}");
        assert!(msg.contains("缺必需列"), "{msg}");
        assert!(msg.contains("\"sharpe\""), "应点名缺的具体列: {msg}");
        assert!(msg.contains("sortino"), "必须列出实际列名以便定位: {msg}");

        // 4) 未删列时一切照常
        let ok = read_parquet(full.to_str().unwrap()).unwrap();
        assert_eq!(ok[0].metrics.sharpe_ratio, 1.5);
        assert_eq!(ok[0].metrics.sortino_ratio, 2.5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
