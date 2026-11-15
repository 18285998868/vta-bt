//! 参数串规范化与确定性参数编码。

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
/// 唯一性由 [`build_jobs_checked`](crate::build::build_jobs_checked)
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
        // Clenow 混合 "entry=E,exit=X,fast=F,slow=S,mode=M" 比海龟多两个维度:
        // 位打包 E(15b)|X(15b)|F(15b)|S(15b)|M(1b); 超界回退整串 FNV。
        if clean.contains(",fast=") {
            let field = |key: &str| -> Option<u64> {
                clean
                    .split(',')
                    .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
                    .and_then(|v| v.parse().ok())
            };
            let (e, x, f, s) = (field("entry"), field("exit"), field("fast"), field("slow"));
            if let (Some(e), Some(x), Some(f), Some(s)) = (e, x, f, s)
                && [e, x, f, s].iter().all(|v| *v < (1 << 15))
            {
                let mode_bit = u64::from(clean.ends_with("mode=long_short"));
                return (e << 49) | (x << 33) | (f << 17) | (s << 1) | mode_bit;
            }
            return fnv1a_64(&clean);
        }
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
            let idx = crate::bbrk_max::MA_NAMES
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
        let idx = crate::bbrk_bb::BB_MA_NAMES
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

    /// Clenow 混合参数: 四个周期维度 + 方向模式必须两两可区分。
    #[test]
    fn test_params_value_clenow_encoding() {
        let a = strategy_params_value("entry=20,exit=10,fast=50,slow=100,mode=long_short");
        let b = strategy_params_value("entry=20,exit=10,fast=50,slow=100,mode=long_only");
        let c = strategy_params_value("entry=55,exit=20,fast=100,slow=200,mode=long_short");
        assert_ne!(a, b, "方向模式必须进入编码");
        assert_ne!(a, c);
        assert_eq!(a & 1, 1);
        assert_eq!(b & 1, 0);
        // 超界周期回退整串 FNV, 仍须可区分。
        let big = "entry=40000,exit=10,fast=50000,slow=60000,mode=long_short";
        assert_eq!(strategy_params_value(big), fnv1a_64(big));
        assert_ne!(strategy_params_value(big), a);
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
            u64::from(fnv1a_32("base")),
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

    /// 仓位后缀必须同时进入 JSON 与排序编码, 否则 sizing 网格行无法区分。
    #[test]
    fn sizing_suffix_is_reflected_in_json_and_encoding() {
        let p = "period=20;size=fixed,size_fraction_pct=20";
        let js = params_to_json(p);
        assert_eq!(
            js,
            r#"{"period":"20","size":"fixed","size_fraction_pct":"20"}"#
        );
        assert!(
            js.parse::<serde_json::Value>().is_ok(),
            "仓位参数 JSON 非法: {js}"
        );

        // stop 与 sizing 共存时仍只有一个 `;`, 逗号分隔全部键值。
        let both = "period=20;stop_loss_pct=2,size=kelly,size_lookback=50";
        let js = params_to_json(both);
        assert_eq!(
            js,
            r#"{"period":"20","stop_loss_pct":"2","size":"kelly","size_lookback":"50"}"#
        );
        assert!(
            js.parse::<serde_json::Value>().is_ok(),
            "stop+sizing JSON 非法: {js}"
        );

        assert_ne!(
            strategy_params_value("period=20;size=fixed,size_fraction_pct=20"),
            strategy_params_value("period=20;size=fixed,size_fraction_pct=30"),
            "不同 sizing 必须有不同 strategy_params"
        );
        assert_ne!(
            strategy_params_value("period=20"),
            strategy_params_value("period=20;size=fixed,size_fraction_pct=20"),
            "无 sizing 与有 sizing 必须有不同 strategy_params"
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
}
