//! 结构化参数: 文本、JSON 键值与整数编码**同源产出**。
//!
//! 历史实现由 `strategy_params_value` / `params_to_json` 从 `param_str` 文本
//! **反解析**参数: 每新增一种参数形态都要同时改两个解析器; 漏改时整数编码
//! 静默退化为 FNV-32, JSON 键名也可能撒谎。本模块改为策略在产出参数时
//! 同时给出 [`ParamSet`]:
//!
//! - `text`: 冻结的 `params` 列文本(逐字节不变);
//! - `pairs`: JSON 的键值(顺序即输出顺序);
//! - `code`: `strategy_params` 整数。
//!
//! 三者由同一处构造, 不可能互相漂移。
//!
//! # 整数编码布局
//!
//! 未改动的策略族沿用历史槽位公式(见 [`code`] 中的函数文档); 原先落入
//! FNV-32 的多键族改为**定宽位打包**。打包顺序为“首字段在高位”, 例如
//! `bbrk_adx` 的 `sigma×1000:14 | len:12 | adxlen:9 | adx×10:10`。
//!
//! | 策略族 | 布局 |
//! |---|---|
//! | `bbrk_adx` | `sigma×1000:14, len:12, adxlen:9, adx×10:10` |
//! | `bbrk_atr` | `sigma×1000:14, bblen:12, atr1:9, atr2:9, atrthr×1000:13` |
//! | `bbrk_rex` | `sigma×1000:14, bblen:12, rexn:8, rexmult×1000:15` |
//! | `bbrk_trail` | `sigma×1000:14, bblen:12, atrlen:9, trailk×1000:15` |
//! | `bbrk_ttm` | `sigma×1000:14, bblen:12, kc×1000:13, ttmn:7` |
//! | `bbrk_vol` / `bbrk_vol_ema` | `sigma×1000:14, bblen:12, voln:9, volmult×1000:15` |
//! | `macd` | `fast:21, slow:21, signal:21` |
//! | `sgma` | `period:17, poly_order:17` |
//! | `vwap` | `thresh×1000:16` |
//! | `fiali` | 无参数 → `0` |
//! | `ma_slope_atr_gate` | `ma:11, k:11, atr:9, kind:1, z:11, (th+10)×1000:15` |
//! | `ma_atr` | `name:6, len:11, mult×1000:15, atrlen:9` |
//! | `mabb_long` / `mabb_short` | `name:6, bblen:12, sigma×1000:14, stopk×1000:14, atrlen:9` |
//! | `bbrk_max` | `name:4, malen:12, bblen:16` |
//! | `ssf` / `ssf_cd` / `ssf_st` / `ssf_st_cd` | `period:20, poles-2:1` |
//!
//! 定宽打包越界时回退到字段值的 FNV-1a 64(不 panic); 启动期的唯一性自检
//! 仍会拒绝同一策略内的编码碰撞。

use vta_core::engine::{SizingConfig, StopConfig};

/// 一组参数的三种表示: `params` 文本、JSON 键值、`strategy_params` 整数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSet {
    text: String,
    pairs: Vec<(String, String)>,
    code: u64,
}

impl ParamSet {
    /// 开始构造一组参数; `code` 是 [`code`] 模块算出的整数编码。
    #[must_use]
    pub fn builder(code: u64) -> ParamSetBuilder {
        ParamSetBuilder {
            text: None,
            pairs: Vec::new(),
            code,
        }
    }

    /// 冻结的 `params` 列文本。
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// JSON 键值(顺序即输出顺序)。
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// `strategy_params` 整数编码。
    #[must_use]
    pub fn code(&self) -> u64 {
        self.code
    }

    /// 取出文本(供 [`Strategy::param_str`](crate::Strategy::param_str) 的默认实现)。
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }

    /// 产出**合法 JSON 对象**字符串; 值一律为字符串并转义。
    #[must_use]
    pub fn json(&self) -> String {
        let mut out = String::with_capacity(2 + self.pairs.len() * 16);
        out.push('{');
        for (i, (k, v)) in self.pairs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_str(k));
            out.push(':');
            out.push_str(&json_str(v));
        }
        out.push('}');
        out
    }

    /// 追加止损/仓位后缀。
    ///
    /// 文本必须与 [`StopConfig::apply_to`] 逐字节一致; 后缀键值同时进入 JSON。
    /// 有后缀时整数编码改为整串 FNV-1a 64(与历史口径一致); 无后缀时原样返回,
    /// 保证旧结果键不变。
    #[must_use]
    pub fn with_suffix(mut self, stop: &StopConfig, sizing: &SizingConfig) -> Self {
        let text = stop.apply_to(&self.text, sizing);
        if text == self.text {
            return self;
        }
        if let Some((_, suffix)) = text.split_once(';') {
            for kv in suffix.split(',') {
                if let Some((k, v)) = kv.split_once('=') {
                    self.pairs.push((k.to_string(), v.to_string()));
                }
            }
        }
        self.code = fnv1a_64(&text);
        self.text = text;
        self
    }
}

/// [`ParamSet`] 的构造器。
///
/// 默认由 `.kv()` 的键值按 `k=v,k=v` 拼接文本; 下划线形态(`sma_10` /
/// `sgma_p10_m2`)用 `.text()` 显式给出冻结文本, 键值仍只用于 JSON。
#[derive(Debug, Clone)]
pub struct ParamSetBuilder {
    text: Option<String>,
    pairs: Vec<(String, String)>,
    code: u64,
}

impl ParamSetBuilder {
    /// 追加一个键值; 同时进入文本(未显式设置时)与 JSON。
    #[must_use]
    pub fn kv(mut self, key: impl Into<String>, value: impl std::fmt::Display) -> Self {
        self.pairs.push((key.into(), value.to_string()));
        self
    }

    /// 显式指定冻结文本(下划线形态与 `base` 等无 `k=v` 形态)。
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// 完成构造。
    #[must_use]
    pub fn build(self) -> ParamSet {
        let text = self.text.unwrap_or_else(|| {
            let mut out = String::new();
            for (i, (k, v)) in self.pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            out
        });
        ParamSet {
            text,
            pairs: self.pairs,
            code: self.code,
        }
    }
}

/// 各策略族的整数编码公式。
///
/// 每个函数只接受**类型化**参数, 不再从文本反解析。未改动的族保持历史公式
/// 逐位不变; 原先走 FNV 的多键族用 `pack` / `packed` 定宽打包。
pub mod code {
    /// 单周期参数: `period` 直接作为编码(单 MA / `dma_10` 等)。
    #[must_use]
    pub fn period(p: usize) -> u64 {
        p as u64
    }

    /// `f64` 按比例缩放后取整; 非有限/负数/溢出统一映射到 `u64::MAX`。
    ///
    /// 与历史 `f64_to_u64_sorted` 逐位一致, 保证未改动族的编码不变。
    #[must_use]
    pub fn scaled(value: f64, scale: f64) -> u64 {
        super::f64_to_u64_sorted(value * scale)
    }

    /// 双整数维度 `hi * 1_000_000 + lo`(`fast/slow`、`left/right`、`entry/exit`)。
    #[must_use]
    pub fn hi_lo(hi: usize, lo: usize) -> u64 {
        (hi as u64)
            .saturating_mul(1_000_000)
            .saturating_add(lo as u64)
    }

    /// `n=N,<second>=K` → `N * 1_000_000 + round(K * 1000)`。
    #[must_use]
    pub fn n_second(n: usize, second: f64) -> u64 {
        (n as u64)
            .saturating_mul(1_000_000)
            .saturating_add(scaled(second, 1000.0))
    }

    /// 单 `sigma` → `round(sigma * 1000)`。
    #[must_use]
    pub fn sigma_single(sigma: f64) -> u64 {
        scaled(sigma, 1000.0)
    }

    /// 双键 `sigma=S,<second>=K` → `round(S*1000)*1M + round(K*10)`。
    #[must_use]
    pub fn sigma_pair(sigma: f64, second: f64) -> u64 {
        scaled(sigma, 1000.0)
            .saturating_mul(1_000_000)
            .saturating_add(scaled(second, 10.0))
    }

    /// 三键及以上 `sigma=S,...` 的历史折叠公式。
    ///
    /// `rest` 是 sigma 之后的**值文本**(按键序)。历史实现按文本判断槽位:
    /// 纯整数直接取值, 小数乘 10; 折叠结果 `h < 1_000_000` 时返回
    /// `round(S*100)*1M + h`, 否则回退整串 FNV-32。这里逐位复刻, 保证未改动
    /// 的 `bbrk_atr_ema` / `bbrk_don` / ... 编码不变。
    #[must_use]
    pub fn sigma_chain(sigma: f64, rest: &[String], text: &str) -> u64 {
        let s = scaled(sigma, 100.0);
        let mut h = 0u64;
        for value in rest {
            let slot = value
                .parse::<u64>()
                .unwrap_or_else(|_| value.parse::<f64>().map(|v| scaled(v, 10.0)).unwrap_or(0));
            h = h.saturating_mul(1000).saturating_add(slot);
        }
        if h < 1_000_000 {
            s.saturating_mul(1_000_000).saturating_add(h)
        } else {
            u64::from(super::fnv1a_32(text))
        }
    }

    /// `ma=NAME,len=L,sigma=S` → `idx(NAME)*10M + L*10k + round(S*1000)`。
    ///
    /// `sigma=None` 用于没有 sigma 维度的 `ma_atr` 旧口径(历史编码)。
    #[must_use]
    pub fn ma_bb(name_idx: usize, len: usize, sigma: Option<f64>) -> u64 {
        (name_idx as u64)
            .saturating_mul(10_000_000)
            .saturating_add((len as u64).saturating_mul(10_000))
            .saturating_add(sigma.map_or(0, |s| scaled(s, 1000.0)))
    }

    /// `ma=NAME,malen=L` → `idx(NAME)*1M + L`(`bbrk_max` 旧口径)。
    #[must_use]
    pub fn ma_malen(name_idx: usize, malen: usize) -> u64 {
        (name_idx as u64)
            .saturating_mul(1_000_000)
            .saturating_add(malen as u64)
    }

    /// `stop=S` → `round(S * 100)`。
    #[must_use]
    pub fn stop_pct(stop: f64) -> u64 {
        scaled(stop, 100.0)
    }

    /// `k=K` → `round(K * 1000)`。
    #[must_use]
    pub fn k_mult(k: f64) -> u64 {
        scaled(k, 1000.0)
    }

    /// `buy=B,sell=S` → `round(B*10)*10000 + round(S*10)`。
    #[must_use]
    pub fn buy_sell(buy: f64, sell: f64) -> u64 {
        scaled(buy, 10.0)
            .saturating_mul(10_000)
            .saturating_add(scaled(sell, 10.0))
    }

    /// Clenow 混合参数: `entry`/`exit`/`fast`/`slow` 各 15 位 + 方向 1 位。
    ///
    /// 任一周期 `>= 1<<15` 时回退整串 FNV-64(历史口径)。
    #[must_use]
    pub fn clenow(
        entry: usize,
        exit: usize,
        fast: usize,
        slow: usize,
        long_short: bool,
        text: &str,
    ) -> u64 {
        let (e, x, f, s) = (entry as u64, exit as u64, fast as u64, slow as u64);
        if [e, x, f, s].iter().all(|v| *v < (1 << 15)) {
            (e << 49) | (x << 33) | (f << 17) | (s << 1) | u64::from(long_short)
        } else {
            super::fnv1a_64(text)
        }
    }

    /// 均线名单中的下标; 配置校验保证名字合法, 未知名字回退 0(与历史一致)。
    #[must_use]
    pub fn name_index(names: &[&str], name: &str) -> usize {
        names.iter().position(|n| *n == name).unwrap_or(0)
    }

    /// 定宽打包: 首字段在高位, 总位宽不得超过 64。
    ///
    /// 任一字段越界(含 `u64::MAX` 的非法值哨兵)时返回 `None`。
    #[must_use]
    pub fn pack(fields: &[(u64, u32)]) -> Option<u64> {
        let mut out = 0u64;
        let mut bits = 0u32;
        for &(value, width) in fields {
            if width == 0 || width >= 64 || value >= (1u64 << width) {
                return None;
            }
            out = (out << width) | value;
            bits += width;
            if bits > 64 {
                return None;
            }
        }
        Some(out)
    }

    /// [`pack`] 的不可失败版本: 越界时回退到字段值的 FNV-1a 64。
    ///
    /// 正常网格(配置校验范围内)总是命中打包分支; 回退只服务无上界的
    /// `macd` / `sgma` 等参数, 仍由启动期唯一性自检兜底。
    #[must_use]
    pub fn packed(fields: &[(u64, u32)]) -> u64 {
        pack(fields).unwrap_or_else(|| {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for &(value, width) in fields {
                for b in value.to_le_bytes().into_iter().chain(width.to_le_bytes()) {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            h
        })
    }
}

/// `f64` → `u64` 的**显式**转换(不依赖 `as` 的饱和语义)。
///
/// 负数/非有限/溢出统一编码到 `u64::MAX`, 避免与合法的 `0` 撞值。
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

/// FNV-1a 32。仅用于 [`code::sigma_chain`] 的历史回退分支。
fn fnv1a_32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a 64。用于带止损后缀的参数串与 Clenow 超界回退。
fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 极简 JSON 字符串字面量: 加引号 + 转义 `"` `\` 与控制字符。
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use vta_core::engine::{AtrStopConfig, BreakevenConfig, SizingConfig, StopConfig};

    #[test]
    fn builder_derives_text_and_json_from_pairs() {
        let p = ParamSet::builder(2_500_000_200)
            .kv("sigma", 2.5)
            .kv("bblen", 20)
            .build();
        assert_eq!(p.text(), "sigma=2.5,bblen=20");
        assert_eq!(p.json(), r#"{"sigma":"2.5","bblen":"20"}"#);
        assert_eq!(p.code(), 2_500_000_200);
    }

    #[test]
    fn empty_pairs_json_is_empty_object() {
        let p = ParamSet::builder(0).text("base").build();
        assert_eq!(p.text(), "base");
        assert_eq!(p.json(), "{}");
    }

    #[test]
    fn pack_is_big_endian_and_rejects_overflow() {
        assert_eq!(code::pack(&[(1, 2), (2, 2)]), Some(0b01_10));
        assert_eq!(code::pack(&[(4, 2)]), None);
        assert_eq!(code::pack(&[(0, 0)]), None);
        assert_eq!(code::pack(&[(0, 64)]), None);
        assert_eq!(code::pack(&[(0, 32), (0, 33)]), None);
    }

    #[test]
    fn legacy_slot_encodings_are_bit_identical() {
        assert_eq!(code::period(20), 20);
        assert_eq!(code::hi_lo(10, 20), 10_000_020);
        assert_eq!(code::n_second(5, 0.2), 5_000_200);
        assert_eq!(code::sigma_single(2.5), 2500);
        assert_eq!(code::sigma_pair(2.5, 20.0), 2_500_000_200);
        assert_eq!(
            code::sigma_chain(
                1.0,
                &["20".to_string(), "0.8".to_string()],
                "sigma=1,bblen=20,exitb=0.8"
            ),
            100_020_008
        );
        assert_eq!(code::ma_bb(0, 20, Some(2.5)), 202_500);
        assert_eq!(code::ma_bb(2, 5, Some(3.0)), 20_053_000);
        assert_eq!(code::ma_bb(0, 10, None), 100_000);
        assert_eq!(code::ma_malen(7, 100), 7_000_100);
        assert_eq!(code::stop_pct(0.5), 50);
        assert_eq!(code::k_mult(1.5), 1500);
        assert_eq!(code::buy_sell(10.0, 90.0), 1_000_900);
    }

    #[test]
    fn clenow_keeps_mode_bit_and_falls_back_on_large_periods() {
        let text = "entry=20,exit=10,fast=50,slow=100,mode=long_short";
        let a = code::clenow(20, 10, 50, 100, true, text);
        let b = code::clenow(20, 10, 50, 100, false, text);
        assert_eq!(a & 1, 1);
        assert_eq!(b & 1, 0);
        assert_ne!(a, b);
        let c = code::clenow(55, 20, 100, 200, true, text);
        assert_ne!(a, c);
        let big = "entry=40000,exit=10,fast=50000,slow=60000,mode=long_short";
        assert_eq!(
            code::clenow(40000, 10, 50000, 60000, true, big),
            fnv1a_64(big)
        );
    }

    #[test]
    fn name_index_uses_declared_order() {
        assert_eq!(code::name_index(&["sma", "ema", "hma"], "hma"), 2);
        assert_eq!(code::name_index(&["sma", "ema"], "unknown"), 0);
    }

    #[test]
    fn negative_and_non_finite_values_stay_distinct_from_zero() {
        assert_eq!(code::k_mult(-1.5), u64::MAX);
        assert_ne!(code::k_mult(-1.5), code::k_mult(0.0));
        assert_eq!(code::sigma_pair(f64::NAN, 20.0), u64::MAX);
    }

    #[test]
    fn broad_vol_grid_has_no_collisions() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for s in [1.5f64, 2.0, 2.5, 3.0, 3.5, 4.0] {
            for l in [10u64, 20, 30, 40, 50, 60] {
                for n in [5u64, 10, 15, 20] {
                    for m in [1.2f64, 1.5, 2.0, 2.5, 3.0] {
                        let value = code::packed(&[
                            (code::scaled(s, 1000.0), 14),
                            (l, 12),
                            (n, 9),
                            (code::scaled(m, 1000.0), 15),
                        ]);
                        assert!(seen.insert(value), "collision: {s}/{l}/{n}/{m}");
                    }
                }
            }
        }
        assert_eq!(seen.len(), 720);
    }

    #[test]
    fn new_fixed_field_families_are_unique_on_their_grids() {
        use std::collections::HashSet;
        let mut macd = HashSet::new();
        for f in [8usize, 12, 20] {
            for s in [21usize, 26, 34] {
                for g in [5usize, 9, 13] {
                    let value = code::packed(&[(f as u64, 21), (s as u64, 21), (g as u64, 21)]);
                    assert!(macd.insert(value));
                }
            }
        }
        assert_eq!(macd.len(), 27);

        let mut sgma = HashSet::new();
        for p in [10usize, 20, 30] {
            for o in [2usize, 3] {
                let value = code::packed(&[(p as u64, 17), (o as u64, 17)]);
                assert!(sgma.insert(value));
            }
        }
        assert_eq!(sgma.len(), 6);

        let mut gate = HashSet::new();
        for th in [-1.0f64, 0.0, 0.5, 2.0] {
            let value = code::packed(&[
                (100, 11),
                (20, 11),
                (120, 9),
                (0, 1),
                (60, 11),
                (code::scaled(th + 10.0, 1000.0), 15),
            ]);
            assert!(gate.insert(value));
        }
        assert_eq!(gate.len(), 4);
    }

    #[test]
    fn vwap_fixture_thresholds_fit_the_field() {
        for th in [10.0f64, 20.0, 30.0] {
            assert!(code::pack(&[(code::scaled(th, 1000.0), 16)]).is_some());
        }
    }

    #[test]
    fn suffix_is_reflected_in_text_json_and_code() {
        let base = ParamSet::builder(20).kv("period", 20).build();
        let stop = StopConfig {
            stop_loss_pct: Some(2.0),
            trailing_stop_pct: Some(5.0),
            ..StopConfig::default()
        };
        let size = SizingConfig::Fixed { fraction_pct: 20.0 };
        let p = base.clone().with_suffix(&stop, &size);
        assert_eq!(
            p.text(),
            "period=20;stop_loss_pct=2,trailing_stop_pct=5,size=fixed,size_fraction_pct=20"
        );
        assert_eq!(
            p.json(),
            r#"{"period":"20","stop_loss_pct":"2","trailing_stop_pct":"5","size":"fixed","size_fraction_pct":"20"}"#
        );
        assert_eq!(p.code(), fnv1a_64(p.text()));
        assert_ne!(p.code(), base.code());
        assert_eq!(p.text(), stop.apply_to(base.text(), &size));

        let no = base
            .clone()
            .with_suffix(&StopConfig::default(), &SizingConfig::AllIn);
        assert_eq!(no, base);
    }

    #[test]
    fn suffix_without_base_pairs_is_legal_json() {
        let base = ParamSet::builder(0).text("base").build();
        let stop = StopConfig {
            atr: Some(AtrStopConfig {
                period: 14,
                mult: 2.0,
                trailing: true,
            }),
            breakeven: Some(BreakevenConfig {
                trigger_pct: 1.0,
                offset_pct: 0.5,
            }),
            ..StopConfig::default()
        };
        let p = base.with_suffix(&stop, &SizingConfig::AllIn);
        assert_eq!(
            p.json(),
            r#"{"atr_period":"14","atr_mult":"2","atr_trailing":"true","breakeven_trigger_pct":"1","breakeven_offset_pct":"0.5"}"#
        );
        assert!(p.json().parse::<serde_json::Value>().is_ok());
    }

    #[test]
    fn json_escapes_control_characters() {
        let p = ParamSet::builder(0).kv("k", "a\"b\\c\n\t\u{1}").build();
        let j = p.json();
        assert!(j.parse::<serde_json::Value>().is_ok(), "{j}");
        assert!(j.contains("\\\""));
        assert!(j.contains("\\\\"));
        assert!(j.contains("\\n"));
        assert!(j.contains("\\t"));
        assert!(j.contains("\\u0001"));
    }
}
