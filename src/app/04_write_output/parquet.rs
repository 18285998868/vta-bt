use std::path::{Path, PathBuf};
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
#[must_use]
pub fn params_to_json(params: &str) -> String {
    let clean = params.replace(' ', "");
    // 止损后缀: `<策略参数>;stop_loss_pct=2,trailing_stop_pct=5`。
    // 先递归解析策略参数(可能是 `ema_3` / `fast=...` / `period=...` 等任意形态),
    // 再把止损键值并入同一个 JSON 对象。
    if let Some((base, stop)) = clean.split_once(';') {
        let base_json = params_to_json(base);
        let mut out = base_json
            .strip_suffix('}')
            .unwrap_or(base_json.as_str())
            .to_string();
        for kv in stop.split(',') {
            if let Some((k, v)) = kv.split_once('=') {
                out.push_str(&format!(",\"{}\":{}", k, json_str(v)));
            }
        }
        out.push('}');
        return out;
    }
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
        "entry=", "left=", "n=", "stop=", "k=", "sigma=", "buy=", "reset=", "ma=",
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
        // MA strategies: "ema_3", "sma_10" —— 末尾数字**就是** period。
        //
        // 但对 `edma_p5_o0` / `sgma_p10_m2` / `atma_u5_d10` 这类**多参数**形态,
        // 末尾数字只是最后一个维度, 写成 `{"period":"0"}` 是**错的**: `edma_p5_o0`
        // 的真实 period 是 5 而不是 0。下游按 period 分组会直接归错类。
        // 故多参数形态按参数串里原有的键名逐个还原(`edma_p5_o0` → {"p","o"})。
        // 只拆一次: 原实现拆两遍(match 的判别式 + `_` 分支各一次)。
        let pairs = split_underscore_pairs(&clean);
        match pairs {
            Some(ref p) if p.len() > 1 => {
                let body = p
                    .iter()
                    .map(|(k, v)| format!("{}:{}", json_str(k), json_str(v)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            _ => {
                let period = pairs
                    .and_then(|p| p.first().map(|(_, v)| v.clone()))
                    .unwrap_or_else(|| trailing_digits(&clean));
                format!("{{\"period\":{}}}", json_str(&period))
            }
        }
    }
}

/// 串末尾的连续 ASCII 数字; 没有则空串。
fn trailing_digits(s: &str) -> String {
    s.chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// 把 `<策略名>_<段>_<段>…` 拆成 `(键, 值)` 序列, 跳过开头的策略名段。
///
/// - `edma_p5_o0` → `[("p","5"), ("o","0")]`(段 = 键字母 + 数字)
/// - `macd_8_21_5` → `[("0","8"), ("1","21"), ("2","5")]`(段是纯数字 ⇒ 键退化为位置)
/// - `sma_10` → `[("0","10")]`(单段, 键退化为位置; 由调用方按单参数形态处理)
///
/// 任一段不含数字即返回 `None`, 交调用方走既有兜底。
fn split_underscore_pairs(s: &str) -> Option<Vec<(String, String)>> {
    let mut parts = s.split('_');
    // 首段是策略名(edma / sgma / sma …), 不携带参数。
    parts.next()?;
    let mut out = Vec::new();
    for seg in parts {
        let split_at = seg.find(|c: char| c.is_ascii_digit()).unwrap_or(seg.len());
        let (key, val) = seg.split_at(split_at);
        if val.is_empty() || !val.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let key = if key.is_empty() {
            out.len().to_string()
        } else {
            key.to_string()
        };
        out.push((key, val.to_string()));
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
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

/// f64 → u64 的**显式**转换(不依赖 `as` 的饱和语义)。
///
/// 为什么要显式: `as` 从 f64 到 u64 是**饱和**的 —— 负数 → 0, NaN → 0。
/// 于是 `sigma = -1.0` 会编码成 0, 与真实的 `sigma = 0` **撞值**: 报告里两个
/// 不同参数在 `strategy_params` 列上不可区分, 排序也失去意义。
/// 启动期的唯一性自检(`build_jobs_checked`, 逐条策略)会把撞值变成启动错误,
/// 但错误信息会指向"参数网格碰撞"这个**误导性**根因。
///
/// 这里把负数与非有限值统一编码到 `u64::MAX`(彼此仍可能撞, 但**不会**与
/// 合法的非负参数撞), 非负值保持与原实现**逐位相同**的编码。
///
/// `v` 必须是**已按槽位缩放**的值(调用方传 `原始值 * 1000.0` 之类), 本函数
/// 只做取整与范围检查, 不再二次缩放。
fn f64_to_u64_sorted(v: f64) -> u64 {
    if !v.is_finite() {
        return u64::MAX;
    }
    let r = v.round();
    if r < 0.0 || r >= u64::MAX as f64 {
        return u64::MAX;
    }
    r as u64
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

/// FNV-1a 64。用于带止损后缀的参数串: 后缀不属于已知槽位编码, 用 64 位
/// 哈希降低 stop 网格在 `strategy_params` 列上的碰撞概率。
fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 参数串 → 可排序的 `u64` 代理编码(报告的 `strategy_params` 列)。
///
/// **同一策略内必须唯一**: 该列是报告里「按参数大小排序 / 取 top-N」的依据。
/// 编码分两种 —— 已知形态按槽位打包, 未识别形态回退 FNV-32(有损)。
/// 唯一性由 [`build_jobs_checked`](crate::strategies::build::build_jobs_checked)
/// 在启动时逐条策略自检, 碰撞即报错。
#[must_use]
pub fn strategy_params_value(params: &str) -> u64 {
    let clean = params.replace(' ', "");
    // 带止损后缀时, 后缀不属于任何槽位编码; 用 FNV-64 覆盖整个字符串。
    // 同一策略内基参数仍由启动期唯一性自检保证; 不同 stop 由后缀区分。
    if clean.contains(';') {
        return fnv1a_64(&clean);
    }
    // 已知形态白名单前缀。走到最后的分支意味着参数串形态未被识别。
    // 该形态不在白名单里时走 FNV 回退 + warn, 而不是静默编码成"末尾数字"
    // (后者在 `period=abc` 之类输入下会退化成 0, 与真实的 period=0 撞值)。
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
    // 摆动点突破 "left=L,right=R" → L*1M+R(与海龟同构, 均为两个 <1M 的整数维度)。
    if let Some(rest) = clean.strip_prefix("left=") {
        let mut it = rest.split(',');
        let l: u64 = it.next().unwrap_or("0").parse().unwrap_or(0);
        let r: u64 = it
            .next()
            .unwrap_or("0")
            .strip_prefix("right=")
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        return l.saturating_mul(1_000_000).saturating_add(r);
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
            .map(|v| f64_to_u64_sorted(v * 1000.0))
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
                        .map(|f| f64_to_u64_sorted(f * 1000.0))
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
            .map(|v| f64_to_u64_sorted(v * 100.0))
            .unwrap_or(0);
    }
    if let Some(rest) = clean.strip_prefix("k=") {
        return rest
            .parse::<f64>()
            .map(|v| f64_to_u64_sorted(v * 1000.0))
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
                .map(|v| f64_to_u64_sorted(v * 100.0))
                .unwrap_or(0);
            let mut h: u64 = 0;
            for kv in it {
                let v = kv.split('=').nth(1).unwrap_or("0");
                let slot: u64 = v.parse::<u64>().unwrap_or_else(|_| {
                    v.parse::<f64>()
                        .map(|f| f64_to_u64_sorted(f * 10.0))
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
                .map(|v| f64_to_u64_sorted(v * 1000.0))
                .unwrap_or(0);
            let t: u64 = rest[comma + 1..]
                .split('=')
                .nth(1)
                .unwrap_or("0")
                .parse::<f64>()
                .map(|v| f64_to_u64_sorted(v * 10.0))
                .unwrap_or(0);
            // 纯 sigma 形如 "2.5" 无逗号才走旧口径；含逗号但解析失败则回落 0(测试锁定)。
            if s != 0 || t != 0 {
                return s.saturating_mul(1_000_000).saturating_add(t);
            }
            return 0;
        }
        return rest
            .parse::<f64>()
            .map(|v| f64_to_u64_sorted(v * 1000.0))
            .unwrap_or(0);
    }
    // RSI2 "buy=B,sell=S" → round(B*10)*10000+round(S*10)。
    if let Some(rest) = clean.strip_prefix("buy=") {
        let mut it = rest.split(',');
        let b: u64 = it
            .next()
            .unwrap_or("0")
            .parse::<f64>()
            .map(|v| f64_to_u64_sorted(v * 10.0))
            .unwrap_or(0);
        let s: u64 = it
            .next()
            .unwrap_or("0")
            .strip_prefix("sell=")
            .unwrap_or("0")
            .parse::<f64>()
            .map(|v| f64_to_u64_sorted(v * 10.0))
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
        // MA strategies: "ema_3", "sma_10" —— 末尾数字**就是**参数身份。
        //
        // 但这个"取末尾数字"的兜底只在串形如 `<前缀>_<数字>` 时才成立。形如
        // `edma_p1_o0` / `sgma_p1_m2` / `atma_u1_d226` / `macd_12_26_9` 的**多参数**
        // 形态, 末尾数字只编码了**最后一个维度**, 直接取它会让整族塌陷成一个值:
        //   edma_p1_o0 … edma_p500_o0  → 全部 0
        //   sgma_p1_m2 … sgma_p500_m2  → 全部 2
        //   atma_u1_d226 / u26_d226 / u51_d226 / … → 20 组各自塌陷
        // 报告的 strategy_params 列正是"同策略内按参数大小排序"的依据, 塌陷后
        // 排序完全失效且无任何提示。
        //
        // 因此只在**严格形状**(`<前缀>_<数字>`, 前缀不含下划线)下取末尾数字;
        // 其余形态一律走 FNV(对整串哈希), 宁可排序代理"无意义"也不静默撞值。
        let split = single_underscore_form(&clean);
        match split {
            Some(v) => v,
            None => {
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

/// `s` 是否形如 `<前缀>_<数字>`(前缀不含下划线), 是则返回该数字。
///
/// 覆盖 `sma_10` / `ema_3` / `t3_12` 这类**单参数**形态 —— 它们末尾的数字就是
/// 参数身份, 可直接作编码。`edma_p1_o0` 含两个下划线、`macd_12_26_9` 含三个,
/// 末尾数字只是其中一个维度, 故返回 `None` 由调用方走 FNV。
fn single_underscore_form(s: &str) -> Option<u64> {
    let (prefix, tail) = s.rsplit_once('_')?;
    // 前缀里再出现下划线 ⇒ 多参数形态, 末尾数字不代表全部维度。
    if prefix.contains('_') {
        return None;
    }
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tail.parse::<u64>().ok()
}

/// 构造结果表的 Arrow `RecordBatch`。
///
/// # Errors
/// Arrow schema 与列数不匹配时返回 [`AppError`](crate::error::AppError)。
///
/// 此前这里用 `.expect("Parquet schema/column count mismatch")` 直接 panic, 而
/// `write_parquet` 的 `# Errors` 段却承诺"schema 与列数不匹配时返回 [`AppError`](crate::error::AppError)"——
/// **文档与实现直接矛盾**。且 `cols` 与 `fields` 是两份手工同步的平行数组
/// (各 51 项), 加一列忘了改另一列就会在**生产路径**上崩掉整轮回测。
fn build_parquet(results: &[RunResult], balance: crate::money::Amount) -> Result<RecordBatch> {
    let mut strategy = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut timeframe = StringBuilder::new();
    let mut strategy_params = UInt64Builder::new();
    let mut strategy_params_json = StringBuilder::new();
    let mut params_full = StringBuilder::new();
    let mut bal = money_builder()?;
    let mut trade_size = StringBuilder::new();
    let mut total_return = Float64Builder::new();
    let mut final_equity = money_builder()?;
    let mut max_dd = Float64Builder::new();
    let mut trades = UInt32Builder::new();
    let mut stop_loss_exits = UInt32Builder::new();
    let mut trailing_stop_exits = UInt32Builder::new();
    let mut sharpe = Float64Builder::new();
    let mut sortino = Float64Builder::new();
    let mut win_rate = Float64Builder::new();
    let mut pf = Float64Builder::new();
    let mut calmar = Float64Builder::new();
    let mut avg_win = money_builder()?;
    let mut avg_loss = money_builder()?;
    let mut expectancy = Float64Builder::new();
    let mut consecutive_wins = UInt32Builder::new();
    let mut consecutive_losses = UInt32Builder::new();
    let mut volatility_pct = Float64Builder::new();
    let mut recovery_factor = Float64Builder::new();
    let mut avg_hold_bars = Float64Builder::new();
    let mut avg_mae_price = Float64Builder::new();
    let mut avg_mfe_price = Float64Builder::new();
    let mut edge_ratio = Float64Builder::new();
    let mut wl_ratio = Float64Builder::new();
    let mut skewness = Float64Builder::new();
    let mut kurtosis = Float64Builder::new();
    let mut sqn = Float64Builder::new();
    let mut pf_stability = Float64Builder::new();
    let mut z_score = Float64Builder::new();
    let mut p_value = Float64Builder::new();
    let mut fixed_pnl = money_builder()?;
    let mut turnover_per_year = Float64Builder::new();
    let mut delay_bars = UInt32Builder::new();
    let mut fee_bps = Float64Builder::new();
    let mut slippage_bps = Float64Builder::new();
    let mut fixed_total_return = Float64Builder::new();
    let mut fixed_sharpe_ratio = Float64Builder::new();
    let mut fixed_max_drawdown = Float64Builder::new();
    let mut fixed_expectancy_bps = Float64Builder::new();
    let mut fixed_profit_factor = Float64Builder::new();
    let mut fill_close = UInt8Builder::new();
    let mut buy_hold_pnl = money_builder()?;
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
        bal.append_value(money_mantissa(balance)?);
        trade_size.append_value("100%");
        total_return.append_value(m.total_return);
        final_equity.append_value(money_mantissa(m.final_equity)?);
        max_dd.append_value(m.max_drawdown_pct / 100.0);
        trades.append_value(m.num_trades);
        stop_loss_exits.append_value(m.stop_loss_exits);
        trailing_stop_exits.append_value(m.trailing_stop_exits);
        sharpe.append_value(m.sharpe_ratio);
        sortino.append_value(m.sortino_ratio);
        win_rate.append_value(m.win_rate / 100.0);
        pf.append_value(m.profit_factor);
        calmar.append_value(m.calmar_ratio);
        avg_win.append_value(money_mantissa(m.avg_win)?);
        avg_loss.append_value(money_mantissa(m.avg_loss)?);
        expectancy.append_value(m.expectancy);
        consecutive_wins.append_value(m.consecutive_wins);
        consecutive_losses.append_value(m.consecutive_losses);
        volatility_pct.append_value(m.volatility);
        recovery_factor.append_value(m.recovery_factor);
        avg_hold_bars.append_value(m.avg_hold_bars);
        avg_mae_price.append_value(m.avg_mae);
        avg_mfe_price.append_value(m.avg_mfe);
        edge_ratio.append_value(m.edge_ratio);
        wl_ratio.append_value(m.wl_ratio);
        skewness.append_value(m.skewness);
        kurtosis.append_value(m.kurtosis);
        sqn.append_value(m.sqn);
        pf_stability.append_value(m.pf_stability);
        z_score.append_value(m.z_score);
        p_value.append_value(m.p_value);
        fixed_pnl.append_value(money_mantissa(m.total_fixed_pnl)?);
        turnover_per_year.append_value(m.turnover_per_year);
        delay_bars.append_value(r.delay_bars);
        fee_bps.append_value(r.fee_bps);
        slippage_bps.append_value(r.slippage_bps);
        fixed_total_return.append_value(m.fixed_total_return);
        fixed_sharpe_ratio.append_value(m.fixed_sharpe_ratio);
        // 除 100 -> 与复利腿 max_drawdown 同为小数(Ratio), 单位错配已修。
        fixed_max_drawdown.append_value(m.fixed_max_drawdown_pct / 100.0);
        fixed_expectancy_bps.append_value(m.fixed_expectancy_bps);
        fixed_profit_factor.append_value(m.fixed_profit_factor);
        fill_close.append_value(u8::from(r.fill_close));
        buy_hold_pnl.append_value(money_mantissa(r.buy_hold_pnl)?);
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
        Arc::new(total_return.finish()),
        Arc::new(final_equity.finish()),
        Arc::new(max_dd.finish()),
        Arc::new(trades.finish()),
        Arc::new(stop_loss_exits.finish()),
        Arc::new(trailing_stop_exits.finish()),
        Arc::new(sharpe.finish()),
        Arc::new(sortino.finish()),
        Arc::new(win_rate.finish()),
        Arc::new(pf.finish()),
        Arc::new(calmar.finish()),
        Arc::new(avg_win.finish()),
        Arc::new(avg_loss.finish()),
        Arc::new(expectancy.finish()),
        Arc::new(consecutive_wins.finish()),
        Arc::new(consecutive_losses.finish()),
        Arc::new(volatility_pct.finish()),
        Arc::new(recovery_factor.finish()),
        Arc::new(avg_hold_bars.finish()),
        Arc::new(avg_mae_price.finish()),
        Arc::new(avg_mfe_price.finish()),
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
        Arc::new(fixed_max_drawdown.finish()),
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
        Field::new(
            "balance",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("trade_size", DataType::Utf8, false),
        Field::new("total_return", DataType::Float64, false),
        Field::new(
            "final_equity",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("max_drawdown", DataType::Float64, false),
        Field::new("trades", DataType::UInt32, false),
        Field::new("stop_loss_exits", DataType::UInt32, false),
        Field::new("trailing_stop_exits", DataType::UInt32, false),
        Field::new("sharpe", DataType::Float64, false),
        Field::new("sortino", DataType::Float64, false),
        Field::new("win_rate", DataType::Float64, false),
        Field::new("profit_factor", DataType::Float64, false),
        Field::new("calmar", DataType::Float64, false),
        Field::new(
            "avg_win",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new(
            "avg_loss",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("expectancy", DataType::Float64, false),
        Field::new("consecutive_wins", DataType::UInt32, false),
        Field::new("consecutive_losses", DataType::UInt32, false),
        Field::new("volatility_pct", DataType::Float64, false),
        Field::new("recovery_factor", DataType::Float64, false),
        Field::new("avg_hold_bars", DataType::Float64, false),
        Field::new("avg_mae_price", DataType::Float64, false),
        Field::new("avg_mfe_price", DataType::Float64, false),
        Field::new("edge_ratio", DataType::Float64, false),
        Field::new("wl_ratio", DataType::Float64, false),
        Field::new("skewness", DataType::Float64, false),
        Field::new("kurtosis", DataType::Float64, false),
        Field::new("sqn", DataType::Float64, false),
        Field::new("pf_stability", DataType::Float64, false),
        Field::new("z_score", DataType::Float64, false),
        Field::new("p_value", DataType::Float64, false),
        Field::new(
            "fixed_pnl",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("params", DataType::Utf8, false),
        Field::new("turnover_per_year", DataType::Float64, false),
        Field::new("delay_bars", DataType::UInt32, false),
        Field::new("fee_bps", DataType::Float64, false),
        Field::new("slippage_bps", DataType::Float64, false),
        Field::new("fixed_total_return", DataType::Float64, false),
        Field::new("fixed_sharpe_ratio", DataType::Float64, false),
        Field::new("fixed_max_drawdown", DataType::Float64, false),
        Field::new("fixed_expectancy_bps", DataType::Float64, false),
        Field::new("fixed_profit_factor", DataType::Float64, false),
        Field::new("fill_close", DataType::UInt8, false),
        Field::new(
            "buy_hold_pnl",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("buy_hold_return", DataType::Float64, false),
        // 退化结果标记: 见 RunStatus。bars_used 与 status 必须配对解读。
        Field::new("bars_used", DataType::UInt32, false),
        Field::new("status", DataType::Utf8, false),
    ];

    let schema = arrow::datatypes::Schema::new(fields);
    Ok(RecordBatch::try_new(Arc::new(schema), cols)?)
}

/// 金额列的 `Decimal128` builder(精度/标度固定)。
///
/// # Errors
/// 精度/标度非法时返回 [`AppError`](crate::error::AppError) ——
/// 本项目的常量 (38, 8) 恒合法, 该分支只是不把 `expect` 写进生产代码。
fn money_builder() -> Result<Decimal128Builder> {
    Ok(Decimal128Builder::new().with_precision_and_scale(MONEY_PRECISION, MONEY_SCALE_I8)?)
}

/// 金额 → Parquet `Decimal128` 尾数。
///
/// # Errors
/// 金额的小数位超过 [`MONEY_SCALE`], 落盘会丢精度 ⇒ 报错(不静默舍入)。
fn money_mantissa(a: crate::money::Amount) -> Result<i128> {
    a.to_decimal128_mantissa(MONEY_SCALE)
}

/// Parquet `Decimal128` 的**精度**(总有效位数)。
const MONEY_PRECISION: u8 = 38;

/// Parquet `Decimal128` 金额列的**标度**(小数位数)。
const MONEY_SCALE: u32 = crate::money::MONEY_SCALE;

/// 同上, `i8` 形式(Arrow 的 `DataType::Decimal128` 用 `i8` 表示标度)。
const MONEY_SCALE_I8: i8 = MONEY_SCALE as i8;

/// 把结果行写成单个 Parquet 文件(父目录自动创建)。
///
/// # Errors
/// IO 失败, 或 Arrow schema 与列数不匹配时返回 [`AppError`](crate::error::AppError)。
pub fn write_parquet(
    path: &Path,
    results: &[RunResult],
    balance: crate::money::Amount,
) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let batch = build_parquet(results, balance)?;
    let schema = batch.schema();

    write_atomic(path, |tmp| {
        let file = File::create(tmp)?;
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    })
}

/// 原子写文件: 先写 `{path}.tmp`, 成功后再 `rename` 到 `path`。
///
/// **为什么必须原子**: `File::create` 直接**截断**目标路径。若在 `write`/
/// `close` 之间进程被杀、磁盘写满或 IO 出错, 目标文件是**半截**的, 而旧的
/// 可用版本已被销毁 —— 此后 `read_parquet` 读到的可能是"行数正好对、内容却
/// 写坏"的中间态, 而缓存有效性检查只看**行数**, 挡不住这种损坏。
///
/// 同目录 `rename` 在 POSIX 上是原子的 ⇒ 目标路径要么是旧的完整文件, 要么是
/// 新的完整文件, **不会**是中间态。
///
/// 临时文件名带 `.tmp` 后缀 ⇒ 扩展名不是 `parquet`, 不会被 `read_dir` 的
/// 过滤条件(`extension() == "parquet"`)选中, 故中途崩溃留下的临时文件不会
/// 被当成数据文件读进来。
///
/// 失败时删除临时文件, 不留垃圾。
fn write_atomic(path: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    match write(&tmp) {
        Ok(()) => {
            std::fs::rename(&tmp, path)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// 把参数串清洗成**文件名安全且无碰撞**的片段。
///
/// 原实现是 `params.replace(' ', "_").replace('=', "-").replace(',', "_")` ——
/// 这**不是**一一映射: `"a=b"` 与 `"a-b"` 都变成 `a-b`; `"a,b"` 与 `"a_b"` 也一样。
/// 于是**不同参数组会写同一个文件、互相覆盖** —— detail 目录里少了一部分曲线,
/// 而 `--pbo` 会基于**少读一半**的曲线算出 φ, 无任何提示。
///
/// 修法: 清洗后的可读片段**追加参数串的 FNV-1a 哈希**(`{h:08x}`，8–16 位十六进制)。
/// 既保留人类可读性, 又保证不同参数串**必定**得到不同文件名。
fn sanitize_params_for_filename(params: &str) -> String {
    let readable: String = params
        .chars()
        .map(|c| match c {
            ' ' => '_',
            '=' => '-',
            ',' => '_',
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+') => c,
            _ => '_',
        })
        .collect();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in params.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{readable}_{h:08x}")
}

/// 写出 `--detail` 的产物: 逐笔交易 + 复利曲线 + (可选) fixed 曲线。
///
/// 每组 (symbol, timeframe, strategy, params) 一个文件组。`write_curves` 为 false
/// 时跳过 GB 级的 fixed 曲线 I/O(做 PBO 时才需开启)。
///
/// 每条曲线/交易文件的 schema metadata 都带 `combo`(产出它的组合缓存 id)与
/// `status`(该档位运行状态); fixed 曲线另用 `time` 列而非下标, 使 PBO 只能
/// 对齐**同一时间网格**的曲线。返回本次实际写出的文件路径, 供调用方写侧车清单。
///
/// # Errors
/// 目录创建、文件写入或 Arrow 构造失败时返回 [`AppError`](crate::error::AppError)。
pub fn write_detail_parquet(
    details: &[crate::app::DetailResult],
    dir: &Path,
    write_curves: bool,
    combo: &str,
) -> Result<Vec<PathBuf>> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::collections::HashMap;
    use std::fs::File;

    let path = dir;
    std::fs::create_dir_all(path)?;
    let mut written = Vec::new();

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
        let clean_params = sanitize_params_for_filename(params);
        let filename = format!("{}_{}_{}_{}.parquet", symbol, tf, strategy, clean_params);
        let filepath = path.join(filename);

        // 同一 params 组只能有一个状态。若出现两个状态, 说明调用方把不同
        // (symbol, tf, strategy, params) 的批次混在一起, 后续 PBO 无法判断
        // 该曲线该不该参与 —— 直接报错, 不静默取第一个。
        let status = group
            .first()
            .map_or(crate::app::RunStatus::Ok, |d| d.status);
        if let Some(bad) = group.iter().find(|d| d.status != status) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 分组 {symbol}/{tf}/{strategy}/{params} 内 status 不一致: {} vs {}",
                status.as_str(),
                bad.status.as_str()
            )));
        }

        // 分组元数据进 schema metadata, 文件名含 params 明文(含 `_` 清洗)故不
        // 做反向解析。两条曲线的 stem 相同, 只有 kind 后缀不同。
        // `combo` 让缓存命中校验能发现"同名文件被另一轮覆盖"; `status` 让
        // `--pbo` 能显式筛掉 no_signals/no_trades/bankrupt。
        // trades 文件也写同一份 metadata: 侧车清单的完整性校验会遍历全部产物。
        let curve_meta = arrow::datatypes::Metadata::new()
            .with("combo", combo.to_string())
            .with("status", status.as_str().to_string())
            .with("symbol", symbol.clone())
            .with("timeframe", tf.clone())
            .with("strategy", strategy.clone())
            .with("params", params.clone());

        let mut trade_entry_time = Int64Builder::new();
        let mut trade_exit_time = Int64Builder::new();
        let mut trade_side = StringBuilder::new();
        let mut trade_entry = Float64Builder::new();
        let mut trade_exit = Float64Builder::new();
        let mut trade_qty = money_builder()?;
        let mut trade_pnl = money_builder()?;
        let mut trade_pnl_pct = Float64Builder::new();
        let mut trade_fixed_pnl = money_builder()?;
        let mut trade_bars_held = Int64Builder::new();
        let mut trade_max_runup = Float64Builder::new();
        let mut trade_max_drawdown = Float64Builder::new();
        let mut trade_exit_reason = StringBuilder::new();

        for d in group {
            for t in &d.trades {
                trade_entry_time.append_value(t.entry_time);
                trade_exit_time.append_value(t.exit_time);
                trade_side.append_value(t.side.as_str());
                trade_entry.append_value(t.entry_price);
                trade_exit.append_value(t.exit_price);
                trade_qty.append_value(money_mantissa(t.qty)?);
                trade_pnl.append_value(money_mantissa(t.pnl)?);
                trade_pnl_pct.append_value(t.pnl_pct);
                trade_fixed_pnl.append_value(money_mantissa(t.fixed_pnl)?);
                trade_bars_held.append_value(t.bars_held);
                trade_max_runup.append_value(t.max_runup);
                trade_max_drawdown.append_value(t.max_drawdown);
                trade_exit_reason.append_value(t.exit_reason);
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
            Field::new(
                "qty",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new(
                "pnl",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new("pnl_pct", DataType::Float64, false),
            Field::new(
                "fixed_pnl",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new("bars_held", DataType::Int64, false),
            Field::new("max_runup", DataType::Float64, false),
            Field::new("max_drawdown", DataType::Float64, false),
            Field::new("exit_reason", DataType::Utf8, false),
        ];

        let trade_schema =
            arrow::datatypes::Schema::new_with_metadata(trade_fields, curve_meta.clone());
        let trade_batch = RecordBatch::try_new(Arc::new(trade_schema), trade_cols)?;

        write_atomic(&filepath, |tmp| {
            let file = File::create(tmp)?;
            let props = WriterProperties::builder().build();
            let mut writer = ArrowWriter::try_new(file, trade_batch.schema(), Some(props))?;
            writer.write(&trade_batch)?;
            writer.close()?;
            Ok(())
        })?;
        written.push(filepath);

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
        written.push(write_curve_file(
            path,
            &stem,
            "equitycurve",
            &curve_meta,
            Field::new("time", DataType::Int64, false),
            Arc::new(bar_time.finish()),
            Field::new("equity", DataType::Float64, false),
            Arc::new(equity.finish()),
        )?);

        // fixed 腿逐 bar 权益: `--pbo` 的 T×N 输入。
        // `write_curves=false`（默认）时跳过: GB 级 I/O 只在做 PBO 时花。
        if !write_curves {
            continue;
        }
        let mut fixed_time = Int64Builder::new();
        let mut fixed_equity = Float64Builder::new();
        for d in group {
            if d.equity_curve.len() != d.fixed_curve.len() {
                return Err(crate::error::AppError::Msg(format!(
                    "detail 分组 {symbol}/{tf}/{strategy}/{params}: equity_curve {} 行 != fixed_curve {} 行",
                    d.equity_curve.len(),
                    d.fixed_curve.len()
                )));
            }
            // 时间必须取自 equity_curve 的同一根 bar; 只写 bar_idx 时,
            // 不同数据窗口的同名参数文件在 PBO 里无法区分, 只能静默按
            // `min(len)` 截断。
            for (&(ts, _), value) in d.equity_curve.iter().zip(d.fixed_curve.iter()) {
                fixed_time.append_value(ts as i64);
                fixed_equity.append_value(*value);
            }
        }
        written.push(write_curve_file(
            path,
            &stem,
            "fixedcurve",
            &curve_meta,
            Field::new("time", DataType::Int64, false),
            Arc::new(fixed_time.finish()),
            Field::new("fixed_equity", DataType::Float64, false),
            Arc::new(fixed_equity.finish()),
        )?);
    }

    Ok(written)
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
) -> Result<PathBuf> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let schema =
        arrow::datatypes::Schema::new_with_metadata(vec![idx_field, val_field], meta.clone());
    let batch = RecordBatch::try_new(Arc::new(schema), vec![idx_col, val_col])?;
    let path = dir.join(format!("{stem}_{kind}.parquet"));
    write_atomic(&path, |tmp| {
        let file = File::create(tmp)?;
        let mut writer = ArrowWriter::try_new(
            file,
            batch.schema(),
            Some(WriterProperties::builder().build()),
        )?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    })?;
    Ok(path)
}

/// 按**列名**取列序号; 缺列时返回错误并列出实际列名。
///
/// 优于魔法列号: 曲线文件的列顺序由 schema 决定, 用常量下标读取会在 schema
/// 调整后**静默读错列**(类型恰好兼容时不报错, 数值静默错位)。
///
/// # Errors
/// schema 中不存在 `name` 列时返回 [`AppError`](crate::error::AppError), 消息里列出实际存在的列名。
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
/// 权益曲线文件元组。
pub struct EquityCurve {
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
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
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列、缺分组元数据, 或列类型不符时
/// 返回 [`AppError`](crate::error::AppError)。
pub fn read_equity_curves(dir: &Path) -> Result<Vec<EquityCurve>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs;
    use std::fs::File;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        crate::error::AppError::Msg(format!("读取 equitycurve 目录失败 {}: {e}", dir.display()))
    })?;
    // 目录项的 IO 错误**不得**被 `filter_map(|e| e.ok())` 吞掉: 那会让
    // "只读到一半曲线"被当成"成功返回", 而 `--pbo` 会基于**不同的策略集合**
    // 算出 φ, 无任何提示。逐项显式传播错误。
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            crate::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet")
            && p.file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_equitycurve"))
        {
            paths.push(p);
        }
    }
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
            times.extend(i64_col(&batch, idx, &path)?);
            values.extend(f64_col(&batch, val, &path)?);
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

/// fixed 曲线文件元组(供 `--pbo` 按 (币, 周期, 策略) 分组)。
#[derive(Debug, Clone)]
pub struct FixedCurve {
    /// 产出该曲线的组合缓存 id(`temps/` 文件名 stem)。
    ///
    /// detail 文件名只含参数串, 不同数据窗口/源码版本会用**同一个文件名**互相
    /// 覆盖。`combo` 记录"这个文件是哪一轮写的", 缓存命中校验与 `--pbo` 索引
    /// 用它发现"索引说 A 轮, 文件其实是 B 轮"的静默错配。
    pub combo: String,
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,
    /// 该档位的运行状态。
    pub status: crate::app::RunStatus,
    /// bar 时间戳(毫秒), 与 [`Self::values`] 等长。
    ///
    /// 取代旧的 `bar_idx`: PBO 只应比较**同一时间网格**上的曲线。仅凭下标
    /// 无法发现两条曲线来自不同数据窗口, 只能按 `min(len)` 静默截断。
    pub times: Vec<i64>,
    /// fixed 腿逐 bar 权益。
    pub values: Vec<f64>,
}

/// detail 目录索引文件名(位于 `detail/manifest.toml`)。
pub const DETAIL_INDEX_FILE: &str = "manifest.toml";
// v2: 索引条目增加 `size` 字段, 读取时校验文件未被截断/替换。
const DETAIL_MANIFEST_VERSION: u32 = 2;

/// 单个组合的 detail 侧车清单: 记录该组合写出的全部文件及大小。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ComboDetailManifest {
    version: u32,
    combo: String,
    files: Vec<DetailArtifact>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DetailArtifact {
    name: String,
    bytes: u64,
}

/// `detail/manifest.toml`: 只列**当前一轮**产出且仍有效的 fixed 曲线。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DetailIndex {
    version: u32,
    entries: Vec<DetailIndexEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DetailIndexEntry {
    name: String,
    combo: String,
    /// 写入索引时的字节数; 读取时校验, 防止文件被截断/替换后仍参与 PBO。
    size: u64,
}

fn combo_id(combo_path: &Path) -> String {
    combo_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn combo_manifest_path(combo_path: &Path) -> PathBuf {
    combo_path.with_extension("detail.toml")
}

/// 只接受"单个普通文件名", 防止索引中的 `../` 把读取带出 detail 目录。
fn is_plain_filename(name: &str) -> bool {
    let mut comps = Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

/// 写入单个组合的 detail 侧车清单。
///
/// 文件已由 [`write_detail_parquet`] 落盘; 这里记录**文件名 + 字节数**, 供
/// 下一次缓存命中时校验"整组产物是否仍在、是否被截断"。清单本身不是缓存
/// 指纹的一部分 —— 缺失/损坏只会触发一次重算, 不会让错误结果继续传播。
///
/// # Errors
/// 任一产物不存在, 或清单写入失败时返回 [`AppError`](crate::error::AppError)。
pub fn write_combo_detail_manifest(
    combo_path: &Path,
    detail_dir: &Path,
    files: &[PathBuf],
) -> Result<()> {
    let combo = combo_id(combo_path);
    let mut artifacts = Vec::with_capacity(files.len());
    for file in files {
        if file.parent() != Some(detail_dir) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 产物不在 detail 目录 {}: {}",
                detail_dir.display(),
                file.display()
            )));
        }
        let meta = std::fs::metadata(file).map_err(|e| {
            crate::error::AppError::Msg(format!("detail 产物写后校验失败 {}: {e}", file.display()))
        })?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("detail 产物没有文件名: {}", file.display()))
            })?;
        if !is_plain_filename(&name) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 产物文件名非法: {name:?}"
            )));
        }
        artifacts.push(DetailArtifact {
            name,
            bytes: meta.len(),
        });
    }
    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    artifacts.dedup_by(|a, b| a.name == b.name);
    let manifest = ComboDetailManifest {
        version: DETAIL_MANIFEST_VERSION,
        combo,
        files: artifacts,
    };
    let text = toml::to_string(&manifest)
        .map_err(|e| crate::error::AppError::Msg(format!("序列化 detail 清单失败: {e}")))?;
    let path = combo_manifest_path(combo_path);
    write_atomic(&path, |tmp| {
        std::fs::write(tmp, text.as_bytes())?;
        Ok(())
    })
}

/// 重算组合前删除旧侧车清单。
///
/// 必须**在写任何 detail 文件之前**调用: 重算会覆盖同名 detail 文件, 但组合
/// parquet 要到最后才写。若中途崩溃而旧清单仍在, 下一轮会把"新 detail + 旧
/// result"误判为完整缓存(文件名、大小、`combo` 都可能相同), 静默产出错配数据。
/// 先删清单 ⇒ 崩溃后下一轮必然重算。
///
/// # Errors
/// 删除失败(非"文件不存在")时返回 [`AppError`](crate::error::AppError)。
pub fn invalidate_combo_detail_manifest(combo_path: &Path) -> Result<()> {
    let path = combo_manifest_path(combo_path);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn read_combo_detail_manifest(combo_path: &Path) -> Result<ComboDetailManifest> {
    let path = combo_manifest_path(combo_path);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "缺少 detail 侧车清单 {}: {e}; 该组合的 detail 产物不完整, 请重跑",
            path.display()
        ))
    })?;
    let manifest: ComboDetailManifest = toml::from_str(&text)?;
    if manifest.version != DETAIL_MANIFEST_VERSION {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 不支持的 detail 清单版本 {}",
            path.display(),
            manifest.version
        )));
    }
    if manifest.combo != combo_id(combo_path) {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 清单 combo={} 与组合文件名不符(文件可能被复制/改名)",
            path.display(),
            manifest.combo
        )));
    }
    Ok(manifest)
}

/// 校验组合缓存对应的 detail 产物是否完整且属于该组合。
///
/// 返回 `Ok(false)` 表"需要重算"(已 `warn!` 说明原因); 只有无法判断的 IO
/// 错误才上抛。校验项:
/// 1. 侧车清单存在、版本/组合 id 匹配;
/// 2. 清单中每个文件存在且字节数未变(截断/半写会被发现);
/// 3. 文件 metadata 的 `combo` 与清单一致(同名文件被另一轮数据窗口覆盖会被发现)。
///
/// # Errors
/// 清单读取遇到非文件不存在的 IO 错误时返回 [`AppError`](crate::error::AppError)。
pub fn detail_cache_is_complete(combo_path: &Path, detail_dir: &Path) -> Result<bool> {
    let combo = combo_id(combo_path);
    let manifest_path = combo_manifest_path(combo_path);
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let manifest: ComboDetailManifest = match toml::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "解析 detail 清单失败 {}: {e}; 重算",
                manifest_path.display()
            );
            return Ok(false);
        }
    };
    if manifest.version != DETAIL_MANIFEST_VERSION || manifest.combo != combo {
        tracing::warn!(
            "detail 清单版本/组合 id 不符 {}; 重算",
            manifest_path.display()
        );
        return Ok(false);
    }
    for art in &manifest.files {
        if !is_plain_filename(&art.name) {
            tracing::warn!("detail 清单含非法文件名 {:?}; 重算", art.name);
            return Ok(false);
        }
        let path = detail_dir.join(&art.name);
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("detail 产物缺失 {}: {e}; 重算", path.display());
                return Ok(false);
            }
        };
        if meta.len() != art.bytes {
            tracing::warn!(
                "detail 产物大小变化 {} (清单 {} B, 实际 {} B); 重算",
                path.display(),
                art.bytes,
                meta.len()
            );
            return Ok(false);
        }
        match curve_combo(&path) {
            Ok(c) if c == combo => {}
            Ok(c) => {
                tracing::warn!(
                    "detail 产物属于另一轮组合 {} (期望 {}); 重算: {}",
                    c,
                    combo,
                    path.display()
                );
                return Ok(false);
            }
            Err(e) => {
                tracing::warn!("detail 产物不可读 {}: {e}; 重算", path.display());
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn curve_combo(path: &Path) -> Result<String> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    builder
        .schema()
        .metadata()
        .get("combo")
        .cloned()
        .ok_or_else(|| {
            crate::error::AppError::Msg(format!(
                "{}: 缺 metadata combo; 该文件可能由旧版本写出, 请删掉后重跑",
                path.display()
            ))
        })
}

/// 汇总本轮全部组合的侧车清单, 写 `detail/manifest.toml`。
///
/// 索引**只列 fixed 曲线**: `--pbo` 只消费它们。目录里上一轮遗留、或参数已从
/// 网格中删除的曲线不会进索引, 因而不会静默参与 CSCV。
///
/// # Errors
/// 任一组合缺侧车清单, 或索引写入失败时返回 [`AppError`](crate::error::AppError)。
pub fn write_detail_index(detail_dir: &Path, combo_paths: &[PathBuf]) -> Result<()> {
    let mut entries = Vec::new();
    for combo_path in combo_paths {
        let manifest = read_combo_detail_manifest(combo_path)?;
        for file in &manifest.files {
            if file.name.ends_with("_fixedcurve.parquet") {
                entries.push(DetailIndexEntry {
                    name: file.name.clone(),
                    combo: manifest.combo.clone(),
                    size: file.bytes,
                });
            }
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.combo.cmp(&b.combo)));
    entries.dedup_by(|a, b| a.name == b.name && a.combo == b.combo);
    let index = DetailIndex {
        version: DETAIL_MANIFEST_VERSION,
        entries,
    };
    let text = toml::to_string(&index)
        .map_err(|e| crate::error::AppError::Msg(format!("序列化 detail 索引失败: {e}")))?;
    let path = detail_dir.join(DETAIL_INDEX_FILE);
    write_atomic(&path, |tmp| {
        std::fs::write(tmp, text.as_bytes())?;
        Ok(())
    })
}

/// 读取目录下全部 `*_fixedcurve.parquet`(一般为 detail/ 目录)。
///
/// 这是**目录扫描**版本, 会把上一轮遗留的多余文件也读进来。`--pbo` 走
/// [`read_fixed_curves_from_manifest`], 只读当前索引列出的文件; 本函数保留给
/// 需要查看目录全貌的调用方/测试。
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列、缺分组元数据, 或列类型不符时
/// 返回 [`AppError`](crate::error::AppError)。
pub fn read_fixed_curves(dir: &Path) -> Result<Vec<FixedCurve>> {
    use std::fs;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        crate::error::AppError::Msg(format!("读取 fixedcurve 目录失败 {}: {e}", dir.display()))
    })?;
    // 同 `read_equity_curves`: 目录项的 IO 错误必须显式传播, 不得被吞掉
    // (否则 `--pbo` 会基于少读一半曲线的集合算 φ, 且无任何提示)。
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            crate::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet")
            && p.file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_fixedcurve"))
        {
            paths.push(p);
        }
    }
    paths.sort();
    for path in paths {
        out.push(read_fixed_curve(&path)?);
    }
    Ok(out)
}

/// 读取 `detail/manifest.toml` 列出的 fixed 曲线(不扫描目录)。
///
/// # Errors
/// 索引缺失/解析失败、索引列出的文件缺失、文件内 `combo` 与索引不一致,
/// 或曲线 schema/metadata 不合法时返回 [`AppError`](crate::error::AppError)。
pub fn read_fixed_curves_from_manifest(dir: &Path) -> Result<Vec<FixedCurve>> {
    let path = dir.join(DETAIL_INDEX_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "缺少 detail 索引 {}: {e}; 请先用 detail=true 且 fixed_curves=true 重跑",
            path.display()
        ))
    })?;
    let index: DetailIndex = toml::from_str(&text)?;
    if index.version != DETAIL_MANIFEST_VERSION {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 不支持的 detail 索引版本 {}",
            path.display(),
            index.version
        )));
    }
    let mut out = Vec::with_capacity(index.entries.len());
    for entry in index.entries {
        if !is_plain_filename(&entry.name) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 索引含非法文件名 {:?}",
                entry.name
            )));
        }
        let file = dir.join(&entry.name);
        let actual = std::fs::metadata(&file)?.len();
        if actual != entry.size {
            return Err(crate::error::AppError::Msg(format!(
                "{}: 索引记录 {} B, 实际 {} B; 文件被截断/替换或目录未同步, 请重跑",
                file.display(),
                entry.size,
                actual
            )));
        }
        let curve = read_fixed_curve(&file)?;
        if curve.combo != entry.combo {
            return Err(crate::error::AppError::Msg(format!(
                "{}: 索引记录 combo={}, 文件内 combo={}; detail 目录可能被另一轮运行覆盖, 请重跑",
                file.display(),
                entry.combo,
                curve.combo
            )));
        }
        out.push(curve);
    }
    Ok(out)
}

fn read_fixed_curve(path: &Path) -> Result<FixedCurve> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let time_idx = col_by_name(&schema, "time", path)?;
    // 按列名取值列, 而非魔法下标 1: 理由同 `read_equity_curves` —— 下标会在
    // schema 调整后静默读到类型恰好兼容的另一列(不报错, 数值错位)。
    let val_idx = col_by_name(&schema, "fixed_equity", path)?;
    let meta = schema.metadata().clone();
    let get = |k: &str| -> Result<String> {
        meta.get(k).cloned().ok_or_else(|| {
            crate::error::AppError::Msg(format!("fixedcurve 缺分组元数据 {k}: {}", path.display()))
        })
    };
    let combo = get("combo")?;
    let symbol = get("symbol")?;
    let timeframe = get("timeframe")?;
    let strategy = get("strategy")?;
    let params = get("params")?;
    let status_text = get("status")?;
    let status = crate::app::RunStatus::parse(&status_text).ok_or_else(|| {
        crate::error::AppError::Msg(format!(
            "{}: 未知 status {status_text:?}; 文件可能由其它版本产出, 请删掉后重跑",
            path.display()
        ))
    })?;
    let reader = builder.build()?;
    let mut times = Vec::new();
    let mut values = Vec::new();
    for batch in reader {
        let batch = batch?;
        times.extend(i64_col(&batch, time_idx, path)?);
        values.extend(f64_col(&batch, val_idx, path)?);
    }
    if times.len() != values.len() {
        return Err(crate::error::AppError::Msg(format!(
            "{}: time 列 {} 行 != fixed_equity 列 {} 行; 文件被截断或损坏",
            path.display(),
            times.len(),
            values.len()
        )));
    }
    Ok(FixedCurve {
        combo,
        symbol,
        timeframe,
        strategy,
        params,
        status,
        times,
        values,
    })
}

/// 列类型不符时的错误消息。
///
/// 可触发场景: `read_parquet` 读的不只是本次写的文件, 还有**历史版本**写的缓存、
/// 别的工具产出、或被手工改过的文件。列一旦类型不符(例如 `total_return` 是
/// Int32 而非 Float64), 原实现直接 panic —— 整轮回测崩在**第一个组合**上,
/// 用户只看到一句 "f64 column", 完全不知道是哪个文件、哪一列出的问题。
fn col_type_err(file: &Path, name: &str, expect: &str, actual: &str) -> crate::error::AppError {
    crate::error::AppError::Msg(format!(
        "{}: 列 {name} 类型不符(期望 {expect}, 实际 {actual}); \
         该文件可能由其它版本或工具产出, 请删掉后重跑",
        file.display()
    ))
}

/// 按列名取列的下标(供类型校验用)。
fn col_name(batch: &RecordBatch, idx: usize) -> String {
    batch
        .schema()
        .fields()
        .get(idx)
        .map_or_else(|| format!("#{idx}"), |f| f.name().clone())
}

/// 列中出现 null 时的错误。
///
/// null 槽位在 `Array::value(i)` 下会**静默**变成该类型的默认值
/// (`Float64` → `0.0`, `Utf8` → `""`, `UInt*` → `0`), 而本项目的报告 schema
/// 全部 `nullable = false` —— 所以 null 只可能来自"别的工具写的文件"或
/// "被手工改过的文件"。那种文件被静默读成默认值时:
/// - `sharpe` 的 null → `0.0` → 与"真实零波动低风险"**不可区分**(这正是
///   `RunStatus` 存在的理由);
/// - `status` 的 null → `""` → 落到 `_ => RunStatus::NoSignals` 分支, 把
///   "正常"误判成"退化"。
///
/// 故 null 一律**报错**, 不静默填默认值。
fn null_err(file: &Path, name: &str, row: usize) -> crate::error::AppError {
    crate::error::AppError::Msg(format!(
        "{}: 列 {name} 第 {row} 行为 null —— 该 schema 声明为非空; \
         文件可能由其它工具产出或被手工改写, 请删掉后重跑",
        file.display()
    ))
}

/// 读一列的**非空**值: null 槽位报错, 不静默填默认值。
fn read_non_null<T, A>(
    batch: &RecordBatch,
    idx: usize,
    file: &Path,
    arr: &A,
    get: impl Fn(&A, usize) -> T,
) -> Result<Vec<T>>
where
    A: Array,
{
    (0..batch.num_rows())
        .map(|i| {
            if arr.is_null(i) {
                return Err(null_err(file, &col_name(batch, idx), i));
            }
            Ok(get(arr, i))
        })
        .collect()
}

fn str_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<String>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Utf8", "非字符串"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i).to_string())
}

fn i64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<i64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Int64", "非 Int64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

fn u64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt64", "非 UInt64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

/// 读一列 `Decimal128` 金额(定点十进制)。
fn dec_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<crate::money::Amount>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Decimal128", "非 Decimal128"))?;
    read_non_null(batch, idx, file, arr, |a, i| {
        crate::money::Amount::from_decimal128_mantissa(a.value(i), MONEY_SCALE)
    })
}

/// 读一列**可选**的 `Decimal128` 金额(旧缓存无此列 ⇒ 缺省零)。
fn opt_dec(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<crate::money::Amount>> {
    match ci.opt(name) {
        Some(i) => dec_col(batch, i, file),
        None => Ok(vec![crate::money::Amount::ZERO; n]),
    }
}

fn f64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<f64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Float64", "非 Float64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

fn u32_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u32>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt32", "非 UInt32"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

fn u8_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u8>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt8", "非 UInt8"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

/// 报告列的**单位**。
///
/// 这张 enum 存在的理由是具体的缺陷, 不是风格偏好:
/// - `mdd` 落盘时**除以了 100**(小数), 而同一张表里的 `fixed_max_drawdown_pct`
///   **没有除**(百分数)。两条腿并排放, 读者无从判断谁是百分数 —— 这是会直接
///   导致结论出错的单位错配。
/// - `volatility` 是百分数, `max_drawdown`/`win_rate` 是小数, 而三个列名里
///   都没有任何单位标记。
///
/// 单位在落盘层是**声明式**的: 见 [`COLUMNS`]。新增列若不声明单位, `cargo test`
/// 会失败, 而不是默默用错单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// 无量纲比率, **小数**: 0.15 = 15%。
    Ratio,
    /// 百分数: 15.0 = 15%。
    Percent,
    /// 货币(以初始资金计价)。
    Money,
    /// 价格差(报价货币), 非货币金额。
    Price,
    /// bar 数。
    Bars,
    /// 次 / 年。
    PerYear,
    /// 整数计数。
    Count,
    /// 基点(bp): 2.0 = 2bp = 0.02%。
    BasisPoints,
    /// 无量纲且非比率(Sharpe / 偏度 / p 值等)。
    Dimensionless,
}

/// 报告列的单位契约。**schema 的单一来源**: 列名 + 单位一一对应。
///
/// 与之配套的换算约定(落盘时):
/// 换算约定(落盘时):
///
/// - `Ratio` 类: `SummaryMetrics` 的 `_pct` 字段**除以 100** 后写入
/// - `Percent` 类: 直接取 `SummaryMetrics` 的 `_pct` 字段
/// - 其余: 按 `SummaryMetrics` 原值写入
///
/// `test_every_report_column_declares_its_unit` 保证本表与 schema 完全一致,
/// 少声明一列就会测试失败, 而不是默默用错单位。
pub const COLUMNS: &[(&str, Unit)] = &[
    ("strategy", Unit::Dimensionless),
    ("symbol", Unit::Dimensionless),
    ("timeframe", Unit::Dimensionless),
    ("strategy_params", Unit::Dimensionless),
    ("strategy_params_json", Unit::Dimensionless),
    ("balance", Unit::Money),
    ("trade_size", Unit::Dimensionless),
    ("total_return", Unit::Ratio),
    ("final_equity", Unit::Money),
    ("max_drawdown", Unit::Ratio),
    ("trades", Unit::Count),
    ("stop_loss_exits", Unit::Count),
    ("trailing_stop_exits", Unit::Count),
    ("sharpe", Unit::Dimensionless),
    ("sortino", Unit::Dimensionless),
    ("win_rate", Unit::Ratio),
    ("profit_factor", Unit::Dimensionless),
    ("calmar", Unit::Dimensionless),
    ("avg_win", Unit::Money),
    ("avg_loss", Unit::Money),
    // `expectancy` 是**逐笔 pnl_pct 的均值**(比率), 不是货币金额 ——
    // 声明成 `Money` 会让读者把它与 `avg_win`/`avg_loss` 混为一谈。
    ("expectancy", Unit::Ratio),
    ("consecutive_wins", Unit::Count),
    ("consecutive_losses", Unit::Count),
    ("volatility_pct", Unit::Percent),
    ("recovery_factor", Unit::Dimensionless),
    ("avg_hold_bars", Unit::Bars),
    ("avg_mae_price", Unit::Price),
    ("avg_mfe_price", Unit::Price),
    ("edge_ratio", Unit::Dimensionless),
    ("wl_ratio", Unit::Dimensionless),
    ("skewness", Unit::Dimensionless),
    ("kurtosis", Unit::Dimensionless),
    ("sqn", Unit::Dimensionless),
    ("pf_stability", Unit::Dimensionless),
    ("z_score", Unit::Dimensionless),
    ("p_value", Unit::Dimensionless),
    ("fixed_pnl", Unit::Money),
    ("params", Unit::Dimensionless),
    ("turnover_per_year", Unit::PerYear),
    ("delay_bars", Unit::Count),
    ("fee_bps", Unit::BasisPoints),
    ("slippage_bps", Unit::BasisPoints),
    ("fixed_total_return", Unit::Ratio),
    ("fixed_sharpe_ratio", Unit::Dimensionless),
    // 单位修正: 原名 fixed_max_drawdown_pct, 值是**百分数**, 而复利腿的
    // max_drawdown 是**小数**。同名不同量纲是最容易读错的一种不一致。
    ("fixed_max_drawdown", Unit::Ratio),
    // 单位是 **bp**(基点), 不是货币: 1.0 = 1bp = 0.01%。
    ("fixed_expectancy_bps", Unit::BasisPoints),
    ("fixed_profit_factor", Unit::Dimensionless),
    ("fill_close", Unit::Dimensionless),
    ("buy_hold_pnl", Unit::Money),
    ("buy_hold_return", Unit::Ratio),
    ("bars_used", Unit::Count),
    ("status", Unit::Dimensionless),
];

/// 按**列名**索引 parquet 列。取代原先 40 个魔法列号。
///
/// 为什么必须按名: `read_parquet` 既读**本次写的**文件也读**历史版本写的**缓存。
/// 用常量下标读取时, 只要列的顺序或数量变化(每追加一列就是一次), 下标就可能
/// 落到类型恰好兼容的另一列上 —— 不报错, 数值**静默错位**(例如把 `sharpe`
/// 读成 `sortino`, 两者都是 `f64`)。按名读取 + 缺列时列出实际列名, 让错配在
/// 第一次发生时就炸掉。
///
/// - [`ColumnIndex::req`] 必须存在, 缺则 `Err` 并列出实际列名。
/// - `ColumnIndex::opt` 可选(旧缓存文件没有该列), 由调用方决定回退值。
#[derive(Debug, Clone)]
struct ColumnIndex {
    map: std::collections::HashMap<String, usize>,
    file: PathBuf,
}

impl ColumnIndex {
    fn new(schema: &arrow::datatypes::Schema, file: &Path) -> Self {
        Self {
            map: schema
                .fields()
                .iter()
                .enumerate()
                .map(|(i, f)| (f.name().clone(), i))
                .collect(),
            file: file.to_path_buf(),
        }
    }

    fn req(&self, name: &str) -> Result<usize> {
        self.map.get(name).copied().ok_or_else(|| {
            let mut actual: Vec<&str> = self.map.keys().map(String::as_str).collect();
            actual.sort_unstable();
            crate::error::AppError::Msg(format!(
                "{} 缺必需列 {name:?}; 实际列: {actual:?}(旧格式缓存请删 temps/ 后重跑)",
                self.file.display()
            ))
        })
    }

    fn opt(&self, name: &str) -> Option<usize> {
        self.map.get(name).copied()
    }
}

/// 只读 Parquet 元数据里的行数, **不解码任何列**。
///
/// 缓存有效性检查(`run_combo`)只需要"行数是否等于参数数"这一个事实; 原先调用
/// [`read_parquet`] 会把整张表(含全部列、全部行)解码成 `Vec<RunResult>`, 而这条
/// 路径对每个策略都要走一次 —— 参数网格越大、缓存命中越多, 浪费越大。
///
/// 这里只打开 footer 读 `num_rows`; 真正的 schema/内容校验仍由后续的
/// [`read_parquet`] / [`merge_parquet_files`] 在读取数据时完成。
///
/// # Errors
/// 文件打开/footer 解析失败, 或行数超出 `usize` 时返回 [`AppError`](crate::error::AppError)。
pub fn parquet_row_count(path: &Path) -> Result<usize> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let rows = builder.metadata().file_metadata().num_rows();
    usize::try_from(rows).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "{}: Parquet 行数 {rows} 无法表示为 usize: {e}",
            path.display()
        ))
    })
}

/// 读取一个组合临时文件/报告文件。
///
/// 兼容策略: `turnover_per_year` 及其后的列都是**历史上逐次追加**的, 旧缓存文件
/// 没有它们。原先用 `batch.num_columns() > 37` 这类**列数**判断, 一旦有人调整列
/// 顺序就会把"有但不同名"的列误判成"没有"。现在统一走 `ColumnIndex::opt`:
/// 按名字找, 找不到才回退。
///
/// # Errors
/// 文件打开/解析失败、缺必需列, 或列类型不符时返回 [`AppError`](crate::error::AppError)。
pub fn read_parquet(path: &Path) -> Result<Vec<RunResult>> {
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
    let total_return_idx = ci.req("total_return")?;
    let final_equity_idx = ci.req("final_equity")?;
    let max_dd_idx = ci.req("max_drawdown")?;
    let trades_idx = ci.req("trades")?;
    let sharpe_idx = ci.req("sharpe")?;
    let sortino_idx = ci.req("sortino")?;
    let win_rate_idx = ci.req("win_rate")?;
    let pf_idx = ci.req("profit_factor")?;
    let calmar_idx = ci.req("calmar")?;
    let avg_win_idx = ci.req("avg_win")?;
    let avg_loss_idx = ci.req("avg_loss")?;
    let expectancy_idx = ci.req("expectancy")?;
    let cw_idx = ci.req("consecutive_wins")?;
    let cl_idx = ci.req("consecutive_losses")?;
    let volatility_idx = ci.req("volatility_pct")?;
    let recovery_idx = ci.req("recovery_factor")?;
    let avg_hold_idx = ci.req("avg_hold_bars")?;
    let avg_mae_idx = ci.req("avg_mae_price")?;
    let avg_mfe_idx = ci.req("avg_mfe_price")?;
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

        let strategy = str_col(&batch, strategy_idx, path)?;
        let symbol = str_col(&batch, symbol_idx, path)?;
        let timeframe = str_col(&batch, timeframe_idx, path)?;
        let params_full = str_col(&batch, params_idx, path)?;
        let pnl = f64_col(&batch, total_return_idx, path)?;
        let eq = dec_col(&batch, final_equity_idx, path)?;
        let mdd = f64_col(&batch, max_dd_idx, path)?;
        let trades = u32_col(&batch, trades_idx, path)?;
        // 旧缓存文件没有止损计数列 → 回退 0(表示"未记录/无止损")。
        let stop_loss_exits = opt_u32(&batch, &ci, "stop_loss_exits", n, path)?;
        let trailing_stop_exits = opt_u32(&batch, &ci, "trailing_stop_exits", n, path)?;
        let sharpe = f64_col(&batch, sharpe_idx, path)?;
        let sortino = f64_col(&batch, sortino_idx, path)?;
        let wr = f64_col(&batch, win_rate_idx, path)?;
        let pf = f64_col(&batch, pf_idx, path)?;
        let calmar = f64_col(&batch, calmar_idx, path)?;
        let avg_win = dec_col(&batch, avg_win_idx, path)?;
        let avg_loss = dec_col(&batch, avg_loss_idx, path)?;
        let expectancy = f64_col(&batch, expectancy_idx, path)?;
        let cw = u32_col(&batch, cw_idx, path)?;
        let cl = u32_col(&batch, cl_idx, path)?;
        let volatility = f64_col(&batch, volatility_idx, path)?;
        let recovery = f64_col(&batch, recovery_idx, path)?;
        let avg_hold = f64_col(&batch, avg_hold_idx, path)?;
        let avg_mae = f64_col(&batch, avg_mae_idx, path)?;
        let avg_mfe = f64_col(&batch, avg_mfe_idx, path)?;
        let edge = f64_col(&batch, edge_idx, path)?;
        let wl = f64_col(&batch, wl_idx, path)?;
        let skew = f64_col(&batch, skew_idx, path)?;
        let kurt = f64_col(&batch, kurt_idx, path)?;
        let sqn = f64_col(&batch, sqn_idx, path)?;
        let pf_stab = f64_col(&batch, pf_stab_idx, path)?;
        let z_score = f64_col(&batch, z_idx, path)?;
        let p_value = f64_col(&batch, pval_idx, path)?;
        let fixed_pnl = dec_col(&batch, fixed_pnl_idx, path)?;

        // 以下均为**历史上逐次追加**的可选列, 旧缓存文件没有 → 回退为旧口径。
        let turnover_per_year = opt_f64(&batch, &ci, "turnover_per_year", n, path)?;
        let delay_bars = opt_u32(&batch, &ci, "delay_bars", n, path)?;
        let fee_bps = opt_f64(&batch, &ci, "fee_bps", n, path)?;
        let slippage_bps = opt_f64(&batch, &ci, "slippage_bps", n, path)?;
        let fixed_total_return = opt_f64(&batch, &ci, "fixed_total_return", n, path)?;
        let fixed_sharpe_ratio = opt_f64(&batch, &ci, "fixed_sharpe_ratio", n, path)?;
        let fixed_max_dd = opt_f64(&batch, &ci, "fixed_max_drawdown", n, path)?;
        let fixed_expectancy_bps = opt_f64(&batch, &ci, "fixed_expectancy_bps", n, path)?;
        let fixed_profit_factor = opt_f64(&batch, &ci, "fixed_profit_factor", n, path)?;
        let fill_close = opt_u8(&batch, &ci, "fill_close", n, path)?;
        let buy_hold_pnl = opt_dec(&batch, &ci, "buy_hold_pnl", n, path)?;
        let buy_hold_return = opt_f64(&batch, &ci, "buy_hold_return", n, path)?;
        // O6: 原样读回写入时的编码, 合并时不重算。旧文件无此列 → 0 / 空串。
        let strategy_params = opt_u64(&batch, &ci, "strategy_params", n, path)?;
        let strategy_params_json = match ci.opt("strategy_params_json") {
            Some(i) => str_col(&batch, i, path)?,
            None => vec![String::new(); n],
        };
        // L5: 退化结果标记。旧文件无此列 → Ok / bars_used = 0(表示"未记录")。
        let bars_used = opt_u32(&batch, &ci, "bars_used", n, path)?;
        let status = match ci.opt("status") {
            Some(i) => str_col(&batch, i, path)?
                .iter()
                .map(|s| match s.as_str() {
                    "ok" => crate::app::RunStatus::Ok,
                    "no_signals" => crate::app::RunStatus::NoSignals,
                    "no_trades" => crate::app::RunStatus::NoTrades,
                    // "该档不可计算"必须**如实**还原, 不能落进下面的兜底分支
                    // 变成 `NoSignals` —— 那正是本项目要消除的"不可计算"与
                    // "无信号"混为一谈。
                    "invalid" => crate::app::RunStatus::Invalid,
                    // 破产是"跑出来了但账户归零", 同样不能落进 `NoSignals`.
                    "bankrupt" => crate::app::RunStatus::Bankrupt,
                    // 未知取值(未来版本新增的 status)⇒ 保守落 `NoSignals`:
                    // 未知状态**不是**"算不出来", 不可臆断。
                    _ => crate::app::RunStatus::NoSignals,
                })
                .collect::<Vec<_>>(),
            None => vec![crate::app::RunStatus::default(); n],
        };

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
                    stop_loss_exits: stop_loss_exits[i],
                    trailing_stop_exits: trailing_stop_exits[i],
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
                    // 落盘单位是 Ratio(小数), SummaryMetrics 内部是百分数
                    fixed_max_drawdown_pct: fixed_max_dd[i] * 100.0,
                    fixed_expectancy_bps: fixed_expectancy_bps[i],
                    fixed_profit_factor: fixed_profit_factor[i],
                },
            });
        }
    }
    Ok(all)
}

/// 可选 `f64` 列; 旧缓存文件无此列时返回长度 `n` 的默认值。
fn opt_f64(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<f64>> {
    match ci.opt(name) {
        Some(i) => f64_col(batch, i, file),
        None => Ok(vec![0.0; n]),
    }
}

/// 可选 `u32` 列。
fn opt_u32(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u32>> {
    match ci.opt(name) {
        Some(i) => u32_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 可选 `u64` 列。
fn opt_u64(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u64>> {
    match ci.opt(name) {
        Some(i) => u64_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 可选 `u8` 列。
fn opt_u8(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<u8>> {
    match ci.opt(name) {
        Some(i) => u8_col(batch, i, file),
        None => Ok(vec![0; n]),
    }
}

/// 把结果行写入单个组合缓存文件(父目录自动创建)。
///
/// # Errors
/// IO 失败时返回 [`AppError`](crate::error::AppError)。
pub fn write_single_parquet(
    results: &[RunResult],
    path: &Path,
    balance: crate::money::Amount,
) -> Result<()> {
    write_parquet(path, results, balance)
}

/// 合并本次运行产生的组合文件(含跳过的缓存文件), 输出为最终的 result.parquet。
/// 仅读取显式传入的路径, 避免聚合 `temps/` 中遗留的旧参数文件。
///
/// 合并若干组合临时文件为单一报告。
///
/// **读失败不再静默**: 原实现只打一行日志然后跳过, 返回值仍是成功,
/// 调用方无从得知丢了几个组合 —— 报告里少了参数却显示"完成"。
/// 现在返回 [`MergeStats`], 失败数进入最终摘要; 全部失败才报错。
///
/// # Errors
/// 全部输入文件都读不出来, 或写出合并报告失败时返回 [`AppError`](crate::error::AppError)。
pub fn merge_parquet_files(
    paths: &[PathBuf],
    output_path: &Path,
    balance: crate::money::Amount,
) -> Result<MergeStats> {
    let mut all: Vec<RunResult> = Vec::new();
    let mut failed = 0u32;
    for path in paths {
        match read_parquet(path) {
            Ok(mut results) => all.append(&mut results),
            Err(e) => {
                tracing::warn!("  [警告] 读取失败 {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    if all.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "合并失败: {} 个组合文件全部无法读取, 未产出 {}",
            failed,
            output_path.display()
        )));
    }
    let total = all.len() as u64;
    write_parquet(output_path, &all, balance)?;
    Ok(MergeStats {
        merged_rows: total,
        failed_files: failed,
        total_files: u32::try_from(paths.len()).unwrap_or(u32::MAX),
    })
}

/// [`merge_parquet_files`] 的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeStats {
    /// 成功合并的行数。
    pub merged_rows: u64,
    /// 读失败被跳过的组合文件数。
    pub failed_files: u32,
    /// 传入的组合文件总数。
    pub total_files: u32,
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

    /// 回归: 负参数**不得**编码成 0(那会与真实的 `= 0` 撞值)。
    ///
    /// 可触发场景: `as` 从 f64 到 u64 是饱和的 —— 负数 → 0。于是 `sigma = -1.0`
    /// 与 `sigma = 0.0` 在 `strategy_params` 列上**完全相同**, 报告里两个不同
    /// 参数不可区分, 按参数排序也失去意义。现在负值统一编码到 `u64::MAX`。
    #[test]
    fn negative_params_do_not_collide_with_zero() {
        let neg = strategy_params_value("k=-1.5");
        let zero = strategy_params_value("k=0.0");
        assert_ne!(neg, zero, "负参数与零参数必须可区分(否则报告列静默错位)");
        assert_eq!(neg, u64::MAX, "负参数应编码到 u64::MAX 区间");
        // 非负参数的编码保持与历史口径**逐位相同**(不能因这次整改而变)。
        assert_eq!(strategy_params_value("k=1.5"), 1500);
        assert_eq!(strategy_params_value("sigma=2.5"), 2500);
    }

    #[test]
    fn test_params_value_new_strategies_unique() {
        // 海龟/DT/RBreaker/ATR带/Keltner/BBMR/RSI2/Aberration 编码各策略内唯一。
        assert_eq!(strategy_params_value("entry=20,exit=10"), 20_000_010);
        assert_eq!(strategy_params_value("entry=55,exit=10"), 55_000_010);
        // 摆动点突破 "left=L,right=R" 与海龟同构: L*1M+R。
        assert_eq!(strategy_params_value("left=10,right=5"), 10_000_005);
        assert_eq!(strategy_params_value("left=3,right=10"), 3_000_010);
        assert_eq!(strategy_params_value("left=20,right=7"), 20_000_007);
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
            status: crate::app::RunStatus::Ok,
            trades: vec![],
            equity_curve: (0..3)
                .map(|i| (1_700_000_000.0 + i as f64, 100.0 + i as f64))
                .collect(),
            fixed_curve: vec![1.0, 2.0, 3.0],
        };
        let dir = std::env::temp_dir().join("vta_test_nocurves");
        let _ = std::fs::remove_dir_all(&dir);
        write_detail_parquet(std::slice::from_ref(&d), &dir, false, "combo-test").unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|f| !f.contains("fixedcurve")), "{names:?}");
        assert!(!names.iter().any(|f| f.contains("fixedcurve")), "{names:?}");
        let dir2 = std::env::temp_dir().join("vta_test_curves");
        let _ = std::fs::remove_dir_all(&dir2);
        write_detail_parquet(std::slice::from_ref(&d), &dir2, true, "combo-test").unwrap();
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
            "left=10,right=5",
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
        assert_eq!(
            params_to_json("left=10,right=5"),
            "{\"left\":\"10\",\"right\":\"5\"}"
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

    /// 止损后缀必须同时进入 JSON 与排序编码, 否则 stop 网格行无法区分。
    #[test]
    fn stop_suffix_is_reflected_in_json_and_encoding() {
        let p = "sma_10;stop_loss_pct=2,trailing_stop_pct=5";
        let js = params_to_json(p);
        assert_eq!(
            js,
            r#"{"period":"10","stop_loss_pct":"2","trailing_stop_pct":"5"}"#
        );
        assert!(
            js.parse::<serde_json::Value>().is_ok(),
            "止损参数 JSON 非法: {js}"
        );
        assert_ne!(
            strategy_params_value("sma_10;stop_loss_pct=2"),
            strategy_params_value("sma_10;stop_loss_pct=3"),
            "不同 stop 必须有不同 strategy_params"
        );
        assert_ne!(
            strategy_params_value("sma_10"),
            strategy_params_value("sma_10;stop_loss_pct=2"),
            "无止损与有止损必须有不同 strategy_params"
        );
        assert_eq!(
            params_to_json("period=20;stop_loss_pct=2"),
            r#"{"period":"20","stop_loss_pct":"2"}"#
        );
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
            status: crate::app::RunStatus::Ok,
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
        write_detail_parquet(&details, &dir, false, "combo-test").unwrap();

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
        // 文件名带参数串的哈希后缀(防碰撞, 见 `sanitize_params_for_filename`)。
        let expected = format!(
            "BTCUSDT_1d_sma_{}_equitycurve.parquet",
            sanitize_params_for_filename("period=10")
        );
        let p = dir.join(&expected);
        let curves = read_equity_curves(&dir).unwrap();
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
        assert!(p.exists(), "应存在 {expected}, 实际文件: {files:?}");
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
            status: crate::app::RunStatus::Ok,
            trades: Vec::new(),
            equity_curve: (0..3)
                .map(|i| (1_700_000_000.0 + i as f64 * 3600.0, 500.0 + i as f64))
                .collect(),
            fixed_curve: vec![500.0, 501.0, 502.0],
        }];
        write_detail_parquet(&details, &dir, true, "combo-test").unwrap();

        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        for (name, expect) in [
            ("_equitycurve.parquet", vec!["time", "equity"]),
            ("_fixedcurve.parquet", vec!["time", "fixed_equity"]),
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

        // detail trades 的货币列必须叫 `pnl`: 旧名 `total_return` 与报告里的
        // **比率**列同名不同量纲(一个是 Decimal 金额, 一个是 f64 小数), 下游
        // 按名字取列会把“这一笔赚了多少钱”当成“总收益率”。
        let trade_path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.extension().is_some_and(|x| x == "parquet")
                    && !p.to_string_lossy().contains("_equitycurve")
                    && !p.to_string_lossy().contains("_fixedcurve")
            })
            .expect("应有 trades parquet");
        let f = std::fs::File::open(&trade_path).unwrap();
        let b = ParquetRecordBatchReaderBuilder::try_new(f).unwrap();
        let trade_cols: Vec<String> = b
            .schema()
            .fields()
            .iter()
            .map(|fld| fld.name().clone())
            .collect();
        assert!(trade_cols.iter().any(|c| c == "pnl"), "{trade_cols:?}");
        assert!(
            !trade_cols.iter().any(|c| c == "total_return"),
            "detail trades 不得再用 total_return 命名金额列: {trade_cols:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// fixed 曲线必须携带 `time` / `status` / `combo`: PBO 靠它们筛状态、对齐
    /// 时间网格、识别"同名文件被另一轮覆盖"。
    #[test]
    fn fixed_curve_carries_time_status_and_combo() {
        use crate::app::DetailResult;

        let dir = std::env::temp_dir().join(format!("vta-fixed-meta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = DetailResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            strategy: "macd".into(),
            params: "fast=12,slow=26".into(),
            status: crate::app::RunStatus::NoTrades,
            trades: Vec::new(),
            equity_curve: (0..3)
                .map(|i| (1_700_000_000.0 + i as f64 * 3600.0, 1000.0 + i as f64))
                .collect(),
            fixed_curve: vec![1000.0, 1001.0, 1002.0],
        };
        let files = write_detail_parquet(std::slice::from_ref(&d), &dir, true, "combo-x").unwrap();
        assert_eq!(files.len(), 3, "trades + equitycurve + fixedcurve");
        let curves = read_fixed_curves(&dir).unwrap();
        assert_eq!(curves.len(), 1);
        let c = &curves[0];
        assert_eq!(c.combo, "combo-x");
        assert_eq!(c.status, crate::app::RunStatus::NoTrades);
        assert_eq!(c.times, vec![1_700_000_000, 1_700_003_600, 1_700_007_200]);
        assert_eq!(c.values, vec![1000.0, 1001.0, 1002.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 侧车清单必须能发现"文件被删"和"同名文件被另一轮覆盖"。
    #[test]
    fn detail_manifest_detects_missing_and_overwritten_artifacts() {
        use crate::app::DetailResult;

        let root = std::env::temp_dir().join(format!("vta-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let detail = root.join("detail");
        let temps = root.join("temps");
        let combo_id = "BTCUSDT_1d_sma_0123456789abcdef";
        let combo_path = temps.join(format!("{combo_id}.parquet"));
        let d = DetailResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=10".into(),
            status: crate::app::RunStatus::Ok,
            trades: Vec::new(),
            equity_curve: (0..4)
                .map(|i| (1_700_000_000.0 + i as f64 * 86_400.0, 100.0 + i as f64))
                .collect(),
            fixed_curve: vec![100.0, 101.0, 102.0, 103.0],
        };
        let files =
            write_detail_parquet(std::slice::from_ref(&d), &detail, true, combo_id).unwrap();
        write_combo_detail_manifest(&combo_path, &detail, &files).unwrap();
        assert!(
            detail_cache_is_complete(&combo_path, &detail).unwrap(),
            "刚写完的产物必须判定为完整"
        );

        // 重算前先删清单: 即使中途崩溃, 下一轮也不得把旧清单当完整缓存。
        invalidate_combo_detail_manifest(&combo_path).unwrap();
        // 幂等: 清单已不存在时再删一次不得报错。
        invalidate_combo_detail_manifest(&combo_path).unwrap();
        assert!(
            !detail_cache_is_complete(&combo_path, &detail).unwrap(),
            "清单被主动失效后必须重算"
        );
        write_combo_detail_manifest(&combo_path, &detail, &files).unwrap();

        let victim = files
            .iter()
            .find(|p| p.to_string_lossy().contains("_fixedcurve"))
            .expect("应有 fixedcurve");
        std::fs::remove_file(victim).unwrap();
        assert!(
            !detail_cache_is_complete(&combo_path, &detail).unwrap(),
            "缺失 fixedcurve 必须判定缓存失效"
        );

        // 同名文件被另一轮(不同 combo)覆盖, 字节数相同 => 只能靠 metadata combo 发现。
        write_detail_parquet(std::slice::from_ref(&d), &detail, true, "another-combo").unwrap();
        assert!(
            !detail_cache_is_complete(&combo_path, &detail).unwrap(),
            "文件归属另一轮时必须判定缓存失效"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 全局索引只列 fixed 曲线, 且读取时校验文件内 combo 与索引一致。
    #[test]
    fn detail_index_lists_only_fixed_curves_and_validates_combo() {
        use crate::app::DetailResult;

        let root = std::env::temp_dir().join(format!("vta-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let detail = root.join("detail");
        let temps = root.join("temps");
        let mk = |combo: &str, status| {
            let d = DetailResult {
                symbol: "BTCUSDT".into(),
                timeframe: "1d".into(),
                strategy: "sma".into(),
                params: format!("period={}", if combo.ends_with('a') { 10 } else { 20 }),
                status,
                trades: Vec::new(),
                equity_curve: (0..3)
                    .map(|i| (1_700_000_000.0 + i as f64 * 86_400.0, 100.0 + i as f64))
                    .collect(),
                fixed_curve: vec![100.0, 101.0, 102.0],
            };
            (d, temps.join(format!("{combo}.parquet")))
        };
        let (d_a, combo_a) = mk("BTCUSDT_1d_sma_aaaaaaaaaaaaaaaa", crate::app::RunStatus::Ok);
        let (d_b, combo_b) = mk(
            "BTCUSDT_1d_sma_bbbbbbbbbbbbbbbb",
            crate::app::RunStatus::NoTrades,
        );
        let files_a = write_detail_parquet(
            std::slice::from_ref(&d_a),
            &detail,
            true,
            "BTCUSDT_1d_sma_aaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let files_b = write_detail_parquet(
            std::slice::from_ref(&d_b),
            &detail,
            true,
            "BTCUSDT_1d_sma_bbbbbbbbbbbbbbbb",
        )
        .unwrap();
        write_combo_detail_manifest(&combo_a, &detail, &files_a).unwrap();
        write_combo_detail_manifest(&combo_b, &detail, &files_b).unwrap();
        write_detail_index(&detail, &[combo_a, combo_b]).unwrap();

        let curves = read_fixed_curves_from_manifest(&detail).unwrap();
        assert_eq!(curves.len(), 2, "索引应列出两条 fixed 曲线");
        assert!(
            curves
                .iter()
                .any(|c| c.combo == "BTCUSDT_1d_sma_aaaaaaaaaaaaaaaa"
                    && c.status == crate::app::RunStatus::Ok)
        );
        assert!(
            curves
                .iter()
                .any(|c| c.combo == "BTCUSDT_1d_sma_bbbbbbbbbbbbbbbb"
                    && c.status == crate::app::RunStatus::NoTrades)
        );
        assert_eq!(curves[0].times.len(), 3);

        // 索引列出的文件缺失 => 必须报错, 不得静默少读一条。
        let victim = files_a
            .iter()
            .find(|p| p.to_string_lossy().contains("_fixedcurve"))
            .expect("组合 A 应有 fixedcurve");
        // 文件被追加一个字节(大小变化) => 索引校验必须报错, 即使 parquet 仍可读。
        let original = std::fs::read(victim).unwrap();
        let mut changed = original.clone();
        changed.push(0);
        std::fs::write(victim, &changed).unwrap();
        assert!(
            read_fixed_curves_from_manifest(&detail).is_err(),
            "索引大小不符必须报错"
        );
        std::fs::write(victim, &original).unwrap();
        assert!(
            read_fixed_curves_from_manifest(&detail).is_ok(),
            "恢复原文件后索引应重新可用"
        );
        std::fs::remove_file(victim).unwrap();
        assert!(
            read_fixed_curves_from_manifest(&detail).is_err(),
            "索引列出的文件缺失必须报错"
        );
        let _ = std::fs::remove_dir_all(&root);
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
            buy_hold_pnl: crate::money::Amount::ZERO,
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
        write_parquet(&p, &rows, crate::money::Amount::from_i64(10_000)).unwrap();

        let back = read_parquet(&p).unwrap();
        assert_eq!(back.len(), 3);
        // 缓存校验只读 footer 行数, 必须与完整解码一致。
        assert_eq!(parquet_row_count(&p).unwrap(), 3);
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

    /// 止损计数列必须真正落盘并按列名读回; 否则报告无法解释 stop 网格的差异。
    #[test]
    fn stop_exit_counts_roundtrip_through_parquet() {
        let r = RunResult {
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "sma_20;stop_loss_pct=2".into(),
            strategy_params: 42,
            strategy_params_json: "{\"period\":\"20\",\"stop_loss_pct\":\"2\"}".into(),
            metrics: SummaryMetrics {
                stop_loss_exits: 7,
                trailing_stop_exits: 3,
                ..SummaryMetrics::default()
            },
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let dir = std::env::temp_dir().join(format!("vta-stop-counts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");
        write_parquet(&p, &[r], crate::money::Amount::from_i64(10_000)).unwrap();
        let back = read_parquet(&p).unwrap();
        assert_eq!(back[0].metrics.stop_loss_exits, 7);
        assert_eq!(back[0].metrics.trailing_stop_exits, 3);
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
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let dir = std::env::temp_dir().join(format!("vta-o6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.parquet");
        let out = dir.join("m.parquet");
        write_parquet(
            &a,
            std::slice::from_ref(&r),
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();
        merge_parquet_files(&[a], &out, crate::money::Amount::from_i64(10_000)).unwrap();
        let back = read_parquet(&out).unwrap();
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
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let full = dir.join("full.parquet");
        write_parquet(
            &full,
            std::slice::from_ref(&r),
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

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
        let err =
            read_parquet(&cut).expect_err("缺 sharpe 列必须报错, 不能把 sortino 当 sharpe 读进来");
        let msg = format!("{err}");
        assert!(msg.contains("缺必需列"), "{msg}");
        assert!(msg.contains("\"sharpe\""), "应点名缺的具体列: {msg}");
        assert!(msg.contains("sortino"), "必须列出实际列名以便定位: {msg}");

        // 4) 未删列时一切照常
        let ok = read_parquet(&full).unwrap();
        assert_eq!(ok[0].metrics.sharpe_ratio, 1.5);
        assert_eq!(ok[0].metrics.sortino_ratio, 2.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L7: 每一列都必须在 [`COLUMNS`] 表里声明单位, 且 schema 与表**完全一致**。
    ///
    /// 新增报告列若忘了声明单位, 本测试失败 —— 单位错配(`mdd` 是小数而
    /// `fixed_max_drawdown_pct` 是百分数)是本项目真实发生过的缺陷。
    #[test]
    fn test_every_report_column_declares_its_unit() {
        let dir = std::env::temp_dir().join(format!("vta-units-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");
        write_parquet(
            &p,
            &[RunResult {
                symbol: "B".into(),
                timeframe: "1d".into(),
                strategy: "sma".into(),
                params: "period=20".into(),
                strategy_params: 20,
                strategy_params_json: "{}".into(),
                metrics: SummaryMetrics::default(),
                delay_bars: 1,
                fee_bps: 0.0,
                slippage_bps: 0.0,
                fill_close: false,
                buy_hold_pnl: crate::money::Amount::ZERO,
                buy_hold_return: 0.0,
                bars_used: 300,
                status: crate::app::RunStatus::Ok,
            }],
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        let f = std::fs::File::open(&p).unwrap();
        let schema = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .schema()
            .clone();
        let actual: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        let declared: Vec<&str> = COLUMNS.iter().map(|(n, _)| *n).collect();

        let undeclared: Vec<&&str> = actual.iter().filter(|a| !declared.contains(a)).collect();
        assert!(undeclared.is_empty(), "这些列没有声明单位: {undeclared:?}");
        let stale: Vec<&&str> = declared.iter().filter(|d| !actual.contains(d)).collect();
        assert!(stale.is_empty(), "单位表里声明了不存在的列: {stale:?}");
        assert_eq!(actual.len(), declared.len(), "列数不一致");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **单位契约的语义断言**: 关键列的声明单位必须与引擎口径一致。
    ///
    /// `test_every_report_column_declares_its_unit` 只校验"每列都有声明",
    /// **不校验声明的单位对不对** —— 所以下表曾经把两个比率/基点列声明成
    /// `Money`, 护栏也照样通过。
    ///
    /// 缺陷现场:
    /// - `expectancy` = `pnl_pct_sum / num_trades` ⇒ **比率**(逐笔 pnl_pct 的
    ///   均值), 声明成 `Money` 会让读者把它与 `avg_win`/`avg_loss`(货币)混为一谈;
    /// - `fixed_expectancy_bps` = `total_fixed_pnl / n / initial * 10_000` ⇒
    ///   **基点**, 声明成 `Money` 更是量纲错误。
    #[test]
    fn semantic_units_match_the_engine_definition() {
        let unit_of = |name: &str| {
            COLUMNS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, u)| *u)
                .unwrap_or_else(|| panic!("列 {name} 未在 COLUMNS 中声明"))
        };
        // 比率类。
        assert_eq!(
            unit_of("expectancy"),
            Unit::Ratio,
            "expectancy 是逐笔 pnl_pct 均值(比率)"
        );
        assert_eq!(unit_of("total_return"), Unit::Ratio);
        assert_eq!(unit_of("win_rate"), Unit::Ratio);
        assert_eq!(unit_of("buy_hold_return"), Unit::Ratio);
        assert_eq!(unit_of("fixed_total_return"), Unit::Ratio);
        assert_eq!(unit_of("max_drawdown"), Unit::Ratio);
        assert_eq!(unit_of("fixed_max_drawdown"), Unit::Ratio);
        // 货币类(真正的金额)。
        assert_eq!(unit_of("balance"), Unit::Money);
        assert_eq!(unit_of("final_equity"), Unit::Money);
        assert_eq!(unit_of("avg_win"), Unit::Money);
        assert_eq!(unit_of("avg_loss"), Unit::Money);
        assert_eq!(unit_of("fixed_pnl"), Unit::Money);
        assert_eq!(unit_of("buy_hold_pnl"), Unit::Money);
        // 基点类。
        assert_eq!(
            unit_of("fixed_expectancy_bps"),
            Unit::BasisPoints,
            "fixed_expectancy_bps 是 bp, 不是货币"
        );
        assert_eq!(unit_of("fee_bps"), Unit::BasisPoints);
        assert_eq!(unit_of("slippage_bps"), Unit::BasisPoints);
        // 价格差 ≠ 货币金额。
        assert_eq!(unit_of("avg_mae_price"), Unit::Price);
        assert_eq!(unit_of("avg_mfe_price"), Unit::Price);
        // 百分数。
        assert_eq!(unit_of("volatility_pct"), Unit::Percent);
        // `return_mdd` 曾与 `recovery_factor` 完全重复(都是 total_return / max_dd);
        // `calmar` 才是 CAGR / max_dd。删除重复列后只保留后两者。
        assert!(
            !COLUMNS.iter().any(|(name, _)| *name == "return_mdd"),
            "return_mdd 是 recovery_factor 的重复列, 不得回到 schema"
        );
        assert_eq!(unit_of("recovery_factor"), Unit::Dimensionless);
        assert_eq!(unit_of("calmar"), Unit::Dimensionless);
    }

    /// **null 槽位必须报错**, 不得被静默读成默认值。
    ///
    /// 缺陷现场: 读取路径一律用 `Array::value(i)`, 而该 API 对 null 槽位返回
    /// **该类型的默认值**(`Float64` → `0.0`, `Utf8` → `""`, `UInt*` → `0`)。
    /// 报告 schema 全部 `nullable = false`, 所以 null 只可能来自"别的工具写的
    /// 文件"或"被手工改过的文件" —— 那种文件里的 `sharpe` null 会被读成 `0.0`,
    /// 与"真实零波动低风险"**不可区分**(这正是 `RunStatus` 存在的理由);
    /// `status` 的 null 会被读成 `""` 并落到 `NoSignals` 分支, 把"正常"误判
    /// 成"退化"。
    #[test]
    fn null_cells_are_rejected_instead_of_read_as_defaults() {
        use arrow::array::{Float64Builder, RecordBatch};
        use parquet::arrow::ArrowWriter;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        use parquet::file::properties::WriterProperties;
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("vta-null-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("null.parquet");

        // 1. 先写一个**合法**文件, 拿到真实的 schema 与全部列。
        write_parquet(
            &p,
            &[RunResult {
                symbol: "B".into(),
                timeframe: "1d".into(),
                strategy: "sma".into(),
                params: "period=20".into(),
                strategy_params: 20,
                strategy_params_json: "{}".into(),
                metrics: SummaryMetrics::default(),
                delay_bars: 1,
                fee_bps: 0.0,
                slippage_bps: 0.0,
                fill_close: false,
                buy_hold_pnl: crate::money::Amount::ZERO,
                buy_hold_return: 0.0,
                bars_used: 300,
                status: crate::app::RunStatus::Ok,
            }],
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

        // 2. 读回来, 把 `sharpe` 列换成含 null 的版本(其它列原样)。
        let f = std::fs::File::open(&p).unwrap();
        let mut batches = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .build()
            .unwrap();
        let batch = batches.next().unwrap().unwrap();
        let old_schema = batch.schema().clone();
        let sharpe_idx = old_schema.index_of("sharpe").unwrap();
        let mut fb = Float64Builder::new();
        for i in 0..batch.num_rows() {
            if i == 0 {
                fb.append_null();
            } else {
                fb.append_value(0.0);
            }
        }
        let mut cols: Vec<Arc<dyn arrow::array::Array>> = batch.columns().to_vec();
        cols[sharpe_idx] = Arc::new(fb.finish());
        // 该列在 schema 里必须声明为 **nullable**, 否则 Arrow 会在写入时
        // 直接拒绝(null 与"非空"声明矛盾) —— 而这正是我们要模拟的
        // "别的工具产出的文件"的样子: 它自己声明了 nullable。
        let fields: Vec<arrow::datatypes::Field> = old_schema
            .fields()
            .iter()
            .map(|f| {
                if f.name() == "sharpe" {
                    arrow::datatypes::Field::new(f.name(), f.data_type().clone(), true)
                } else {
                    f.as_ref().clone()
                }
            })
            .collect();
        let schema = Arc::new(arrow::datatypes::Schema::new(fields));
        let bad = RecordBatch::try_new(schema, cols).unwrap();

        // 3. 写出去(模拟"别的工具产出的文件"里有 null)。
        {
            let file = std::fs::File::create(&p).unwrap();
            let mut w = ArrowWriter::try_new(
                file,
                bad.schema(),
                Some(WriterProperties::builder().build()),
            )
            .unwrap();
            w.write(&bad).unwrap();
            w.close().unwrap();
        }

        // 4. 读取必须失败, 且错误消息指明是 null 与哪一列。
        let e = read_parquet(&p).expect_err("含 null 的文件必须被拒");
        let msg = format!("{e}");
        assert!(msg.contains("null"), "错误消息应说明是 null: {msg}");
        assert!(msg.contains("sharpe"), "错误消息应指明列名: {msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **回归**: detail 文件名必须**无碰撞**。
    ///
    /// 可触发场景: 原清洗规则 `replace(' ', "_").replace('=', "-").replace(',', "_")`
    /// **不是**一一映射 —— `"a=b"` 与 `"a-b"` 都变成 `a-b`。于是不同参数组
    /// 写同一个文件、**互相覆盖**, detail 目录少了一部分曲线, 而 `--pbo`
    /// 会基于少读一半的曲线算出 φ, 无任何提示。
    #[test]
    fn detail_filenames_never_collide() {
        // 这些参数串经旧清洗规则会得到**相同**的文件名。
        let colliding = ["a=b", "a-b", "a b", "a,b", "a_b"];
        let mut seen = std::collections::HashSet::new();
        for p in colliding {
            let f = sanitize_params_for_filename(p);
            assert!(
                seen.insert(f.clone()),
                "参数串 {p:?} 与其它参数串撞出了同一个文件名 {f:?}"
            );
        }
        // 不同参数串 ⇒ 不同文件名。
        assert_ne!(
            sanitize_params_for_filename("sigma=2.5"),
            sanitize_params_for_filename("sigma=2.6")
        );
        // 可读性保留: 清洗后的片段仍含原参数的主要信息。
        let f = sanitize_params_for_filename("sigma=2.5,period=20");
        assert!(
            f.starts_with("sigma-2.5_period-20"),
            "文件名应保留可读部分: {f}"
        );
        // 路径分隔符等必须被替换掉, 不得进文件名。
        for p in ["a/b", "a\\b", "a:b", "a*b", "a?b"] {
            let f = sanitize_params_for_filename(p);
            assert!(
                !f.contains('/') && !f.contains('\\') && !f.contains(':'),
                "文件名不得含路径分隔符: {f}"
            );
        }
    }

    /// **回归**: `status = "invalid"` / `"bankrupt"` 必须**如实**往返。
    ///
    /// 可触发场景: 读回时未知的 status 取值会落进 `_ =>` 兜底分支变成
    /// `NoSignals` —— 于是"这一档**没算出来**"会被还原成"跑了但没信号",
    /// 正是本项目要消除的混为一谈。
    #[test]
    fn invalid_status_survives_the_parquet_round_trip() {
        let dir = std::env::temp_dir().join(format!("vta-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");

        let mk = |status: crate::app::RunStatus| RunResult {
            symbol: "B".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            strategy_params: 20,
            strategy_params_json: "{}".into(),
            metrics: SummaryMetrics::default(),
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: 300,
            status,
        };
        write_parquet(
            &p,
            &[
                mk(crate::app::RunStatus::Ok),
                mk(crate::app::RunStatus::NoSignals),
                mk(crate::app::RunStatus::NoTrades),
                mk(crate::app::RunStatus::Invalid),
                mk(crate::app::RunStatus::Bankrupt),
            ],
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

        let back = read_parquet(&p).expect("必须能读回");
        assert_eq!(back.len(), 5);
        assert_eq!(back[0].status, crate::app::RunStatus::Ok);
        assert_eq!(back[1].status, crate::app::RunStatus::NoSignals);
        assert_eq!(back[2].status, crate::app::RunStatus::NoTrades);
        assert_eq!(
            back[3].status,
            crate::app::RunStatus::Invalid,
            "invalid 必须如实还原, 不得落进 _ => 兜底变成 no_signals"
        );
        assert_eq!(
            back[4].status,
            crate::app::RunStatus::Bankrupt,
            "bankrupt 必须如实还原"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **原子写**: 写入失败时, 目标文件必须保持**旧内容**不变。
    ///
    /// 缺陷现场: `File::create` 直接截断目标路径。若在 `write`/`close` 之间
    /// 出错(磁盘满、IO 错误、进程被杀), 目标文件是**半截**的, 而旧的可用版本
    /// 已被销毁; 缓存有效性检查只看**行数**, 挡不住"行数对、内容坏"的中间态。
    ///
    /// 修法: 先写 `{path}.tmp`, 成功后再 `rename`(POSIX 原子)。
    #[test]
    fn failed_write_leaves_the_previous_file_intact() {
        let dir = std::env::temp_dir().join(format!("vta-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");

        // 先写一个成功的版本。
        write_parquet(&p, &[], crate::money::Amount::from_i64(10_000)).unwrap();
        let before = std::fs::read(&p).unwrap();
        assert!(!before.is_empty());

        // 一次失败的写: 闭包返回 Err。
        let e = write_atomic(&p, |_tmp| {
            Err(crate::error::AppError::Msg("故意失败".into()))
        })
        .expect_err("写入必须失败");
        assert!(format!("{e}").contains("故意失败"));

        // 旧文件必须**逐字节**不变。
        let after = std::fs::read(&p).unwrap();
        assert_eq!(
            before.len(),
            after.len(),
            "失败后目标文件被截断了(原子性丢失)"
        );
        assert_eq!(before, after, "失败后目标文件内容被破坏(原子性丢失)");

        // 临时文件必须被清理, 不留垃圾。
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件未被清理: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **原子写**: 成功时目标文件被完整替换, 且不残留 `.tmp`。
    #[test]
    fn atomic_write_replaces_the_target_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("vta-atomic2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");

        write_atomic(&p, |tmp| {
            std::fs::write(tmp, b"hello")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"hello");

        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["r.parquet".to_string()], "不应残留临时文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L7 的核心断言: 复利腿与 fixed 腿的**最大回撤单位一致**(都是小数)。
    ///
    /// 缺陷现场: `mdd` 落盘 ÷100(小数), `fixed_max_drawdown_pct` 未 ÷100(百分数)。
    /// 两条腿并排对比时, fixed 腿的数值会凭空大 100 倍。
    #[test]
    fn test_drawdown_columns_share_the_same_unit() {
        let m = SummaryMetrics {
            max_drawdown_pct: 25.0,       // 百分数
            fixed_max_drawdown_pct: 10.0, // 百分数
            ..SummaryMetrics::default()
        };
        let r = RunResult {
            symbol: "B".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            strategy_params: 20,
            strategy_params_json: "{}".into(),
            metrics: m,
            delay_bars: 1,
            fee_bps: 0.0,
            slippage_bps: 0.0,
            fill_close: false,
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.0,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let dir = std::env::temp_dir().join(format!("vta-dd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");
        write_parquet(
            &p,
            std::slice::from_ref(&r),
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

        use arrow::record_batch::RecordBatch;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        let f = std::fs::File::open(&p).unwrap();
        let mut it = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .build()
            .unwrap();
        let batch: RecordBatch = it.next().unwrap().unwrap();
        let get = |name: &str| -> f64 {
            let i = batch.schema().index_of(name).unwrap();
            f64_col(&batch, i, &p).unwrap()[0]
        };
        let comp = get("max_drawdown");
        let fixed = get("fixed_max_drawdown");
        assert!(
            (comp - 0.25).abs() < 1e-12,
            "复利腿回撤应为 0.25, 实为 {comp}"
        );
        assert!(
            (fixed - 0.10).abs() < 1e-12,
            "fixed 腿回撤应为 0.10(与复利腿同单位), 实为 {fixed}"
        );
        // 往返: 读回时单位必须还原
        let back = read_parquet(&p).unwrap();
        assert!((back[0].metrics.max_drawdown_pct - 25.0).abs() < 1e-9);
        assert!((back[0].metrics.fixed_max_drawdown_pct - 10.0).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 其余 Percent 列也必须保持百分数, 不被"顺手改成小数"。
    #[test]
    fn test_percent_and_ratio_columns_are_not_swapped() {
        let m = SummaryMetrics {
            volatility: 42.0,
            max_drawdown_pct: 25.0,
            win_rate: 60.0,
            total_return_pct: -30.0,
            total_return: -0.30,
            ..SummaryMetrics::default()
        };
        let r = RunResult {
            symbol: "B".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: "period=20".into(),
            strategy_params: 20,
            strategy_params_json: "{}".into(),
            metrics: m,
            delay_bars: 1,
            fee_bps: 2.0,
            slippage_bps: 1.0,
            fill_close: false,
            buy_hold_pnl: crate::money::Amount::ZERO,
            buy_hold_return: 0.4,
            bars_used: 300,
            status: crate::app::RunStatus::Ok,
        };
        let dir = std::env::temp_dir().join(format!("vta-pct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.parquet");
        write_parquet(
            &p,
            std::slice::from_ref(&r),
            crate::money::Amount::from_i64(10_000),
        )
        .unwrap();

        use arrow::record_batch::RecordBatch;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        let f = std::fs::File::open(&p).unwrap();
        let mut it = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .build()
            .unwrap();
        let batch: RecordBatch = it.next().unwrap().unwrap();
        let get = |name: &str| -> f64 {
            let i = batch.schema().index_of(name).unwrap();
            f64_col(&batch, i, &p).unwrap()[0]
        };
        assert!(
            (get("volatility_pct") - 42.0).abs() < 1e-12,
            "volatility 是 Percent"
        );
        assert!(
            (get("max_drawdown") - 0.25).abs() < 1e-12,
            "max_drawdown 是 Ratio"
        );
        assert!((get("win_rate") - 0.60).abs() < 1e-12, "win_rate 是 Ratio");
        assert!(
            (get("total_return") - (-0.30)).abs() < 1e-12,
            "total_return 是 Ratio"
        );
        assert!((get("buy_hold_return") - 0.4).abs() < 1e-12);
        assert!((get("fee_bps") - 2.0).abs() < 1e-12, "fee_bps 是 bp");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
