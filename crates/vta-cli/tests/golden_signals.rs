//! 行为冻结: 全部策略的 `param_str` / `strategy_params` 编码 / 信号指纹。
//!
//! # 为什么需要
//!
//! 本仓库的对外契约不只是"能编译": 报告里的 `params` 文本、`strategy_params`
//! 整数与逐 bar 信号共同决定选参结果。重构(抽网格、抽状态机、拆 crate)只要
//! 动了其中任何一环, 历史结论就会变 —— 而这种变化**不会报错**, 只会让报告
//! 悄悄变成另一套数字。
//!
//! 本测试把三者冻结成一份可 review 的快照:
//! - 每个 `StrategyConfig` 变体一条夹具(`tests/golden/strategies.toml`);
//! - 每档参数一行: `param_str`、编码值、JSON 形态、四组行情的信号指纹。
//!
//! # 如何更新
//!
//! **只在确认行为变化是有意的时候**:
//!
//! ```bash
//! UPDATE_GOLDEN=1 cargo test --test golden_signals
//! git diff tests/golden/expected.txt   # 逐行 review 差异
//! ```
//!
//! 更新后必须把差异原因写进提交说明。CI 不允许更新(该环境变量不会被设置)。
//!
//! # 覆盖边界
//!
//! 信号用**合成行情**计算, 不读外部数据: 测试必须确定、独立、可并行。
//! 真实数据的端到端一致性由 `scripts/golden_run.sh`(release 冒烟 + parquet
//! 逐值比较)负责, 二者互补。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use vta_strategies::StrategySpec;
use vta_strategies::{Bars, ParamOutcome, Signal};

/// 全策略夹具: 每个变体一条 `[[strategy]]`, 网格刻意小(2~3 档/维)。
const FIXTURE: &str = include_str!("golden/strategies.toml");
/// 冻结快照。缺失或为空时先跑一次 `UPDATE_GOLDEN=1`。
const EXPECTED: &str = include_str!("golden/expected.txt");

#[derive(Debug, serde::Deserialize)]
struct Fixture {
    strategy: Vec<StrategySpec>,
}

/// 一组合成行情(六列等长)。
struct Series {
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
    time: Vec<i64>,
}

impl Series {
    fn bars(&self) -> Bars<'_> {
        Bars::new(
            &self.open,
            &self.high,
            &self.low,
            &self.close,
            &self.volume,
            &self.time,
        )
    }
}

/// xorshift64*: 固定种子 ⇒ 固定序列, 不依赖 rand crate 的版本与实现。
fn next_u64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

fn next_unit(state: &mut u64) -> f64 {
    // 取高 53 位 ⇒ [0, 1) 的均匀分布。
    (next_u64(state) >> 11) as f64 / (1u64 << 53) as f64
}

/// 四种行情形态:
/// - `Normal`: 随机游走 + 轻微波动率聚集;
/// - `Gaps`: 与 `Normal` 相同, 但在固定下标注入 `NaN`(指标预热/缺口的常见来源);
/// - `Short`: 只有 40 根, 用来覆盖"周期大于窗口"的可计算性分支;
/// - `Flat`: 常数价, 用来覆盖零带宽/零波动(`pct_b` 的 0/0 分支)。
fn make_series(seed: u64, len: usize, gaps: bool, flat: bool) -> Series {
    let mut state = seed;
    let mut open = Vec::with_capacity(len);
    let mut high = Vec::with_capacity(len);
    let mut low = Vec::with_capacity(len);
    let mut close = Vec::with_capacity(len);
    let mut volume = Vec::with_capacity(len);
    let mut time = Vec::with_capacity(len);

    let mut prev = 100.0_f64;
    for i in 0..len {
        let c = if flat {
            100.0
        } else {
            let drift = (next_unit(&mut state) - 0.5) * 0.02;
            let vol = 1.0 + next_unit(&mut state) * 0.5;
            (prev * (1.0 + drift * vol)).clamp(1.0, 1.0e6)
        };
        let o = if i == 0 { c } else { prev };
        let spread = (next_unit(&mut state) * 0.001).max(1.0e-9);
        open.push(o);
        high.push(o.max(c) * (1.0 + spread));
        low.push(o.min(c) * (1.0 - spread));
        close.push(c);
        volume.push(1_000.0 * (1.0 + next_unit(&mut state)));
        time.push(1_600_000_000_000 + i as i64 * 3_600_000);
        prev = c;
    }

    if gaps {
        // 固定下标 ⇒ 可复现; 同时落在预热区与中段。
        for &i in &[0usize, 37, 120, 211] {
            if i < len {
                close[i] = f64::NAN;
            }
        }
        if len > 121 {
            high[121] = f64::NAN;
        }
    }

    Series {
        open,
        high,
        low,
        close,
        volume,
        time,
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn signal_byte(s: Signal) -> u8 {
    match s {
        Signal::Hold => 0,
        Signal::Buy => 1,
        Signal::Sell => 2,
        Signal::Close => 3,
        Signal::ExitLong => 4,
        Signal::ExitShort => 5,
    }
}

/// 一档参数在一种行情下的指纹。
///
/// `Skipped` 只记 `skip`: 原因文本是日志口径, 不属于行为契约, 冻结它会让
/// 无关的文案调整也触发快照变更。
fn outcome_hash(outcome: &ParamOutcome) -> String {
    match outcome {
        ParamOutcome::Skipped(_) => "skip".to_string(),
        ParamOutcome::Signals(sig) => {
            let mut bytes = Vec::with_capacity(8 + sig.len());
            bytes.extend(u64::try_from(sig.len()).unwrap_or(u64::MAX).to_le_bytes());
            bytes.extend(sig.iter().map(|s| signal_byte(*s)));
            format!("{:016x}", fnv1a(&bytes))
        }
    }
}

fn render() -> String {
    let fixture: Fixture = toml::from_str(FIXTURE).expect("夹具应可解析");
    let series = [
        make_series(0x5eed_1234, 480, false, false),
        make_series(0x5eed_1234, 480, true, false),
        make_series(0x5eed_1234, 40, false, false),
        make_series(0x5eed_1234, 200, false, true),
    ];

    let mut out = String::new();
    out.push_str("# strategy\tidx\tparam_str\tparams_value\tparams_json\tsig_normal\tsig_gaps\tsig_short\tsig_flat\n");
    for spec in &fixture.strategy {
        let job = vta_strategies::build::build_jobs_checked(std::slice::from_ref(spec))
            .expect("夹具中的策略应可构建")
            .pop()
            .expect("每条夹具应产出一个作业");
        let st = job.strategy.as_ref();
        let n = st.param_count();
        assert!(n > 0, "{}: 参数网格不应为空", st.name());
        for idx in 0..n {
            let p = st.param_str(idx);
            let v = vta_strategies::params::strategy_params_value(&p);
            let json = vta_strategies::params::params_to_json(&p);
            let mut hashes = Vec::with_capacity(series.len());
            for s in &series {
                let outcome = st
                    .generate_signals_full(s.bars(), idx)
                    .map(ParamOutcome::Signals)
                    .unwrap_or_else(|e| match e {
                        vta_core::error::AppError::ParamSkipped { reason } => {
                            ParamOutcome::Skipped(reason)
                        }
                        other => panic!("{}#{idx} ({p}): 非预期错误 {other}", st.name()),
                    });
                hashes.push(outcome_hash(&outcome));
            }
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                st.name(),
                idx,
                p,
                v,
                json,
                hashes.join("\t")
            )
            .expect("写入 String 不会失败");
        }
    }
    out
}

/// 夹具必须覆盖 `StrategyConfig` 的每一个变体。
///
/// 新增策略而忘记加夹具时, 上面的快照测试仍然会"通过"(只是少覆盖一个策略),
/// 这条测试就是那道兜底: 变体数 == 夹具条数。
#[test]
fn fixture_covers_every_variant() {
    let src = include_str!("../../vta-strategies/src/config.rs");
    let body = src
        .split("pub enum StrategyConfig {")
        .nth(1)
        .expect("StrategyConfig 应存在")
        .split("\n}")
        .next()
        .expect("枚举体应可截取");

    // serde 的 `rename_all = "snake_case"` 只在"小写/数字之后的大写"前插下划线,
    // 显式 `#[serde(rename = "...")]` 优先。
    let make_snake = |v: &str| -> String {
        let mut s = String::new();
        for ch in v.chars() {
            if ch.is_ascii_uppercase() {
                if s.chars()
                    .last()
                    .is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
                {
                    s.push('_');
                }
                s.push(ch.to_ascii_lowercase());
            } else {
                s.push(ch);
            }
        }
        s
    };

    let mut variants: Vec<String> = Vec::new();
    let mut pending_rename: Option<String> = None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("#[serde(rename = \"") {
            pending_rename = Some(rest.split('"').next().unwrap_or(rest).to_string());
        }
        let Some(v) = t.split('(').next() else {
            continue;
        };
        if v.is_empty() || !v.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        variants.push(pending_rename.take().unwrap_or_else(|| make_snake(v)));
    }
    variants.sort();

    let mut have: Vec<String> = FIXTURE
        .lines()
        .filter_map(|l| l.trim().strip_prefix("type = \""))
        .filter_map(|r| r.split('"').next())
        .map(str::to_string)
        .collect();
    have.sort();
    have.dedup();

    assert_eq!(
        have.len(),
        variants.len(),
        "夹具覆盖 {} 个变体, 枚举有 {} 个 —— 新增策略必须补夹具\n缺: {:?}",
        have.len(),
        variants.len(),
        variants
            .iter()
            .filter(|v| !have.contains(v))
            .collect::<Vec<_>>()
    );
    for v in &variants {
        assert!(
            have.contains(v),
            "变体 {v} 不在 tests/golden/strategies.toml 中"
        );
    }
}

/// 快照比对: 任一行的文本/编码/信号指纹变化都必须让本测试失败。
#[test]
fn golden_snapshot_matches() {
    let actual = render();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/expected.txt"),
            &actual,
        )
        .expect("写入快照应成功");
        eprintln!("已更新 tests/golden/expected.txt(请逐行 review diff)");
        return;
    }

    let expected = EXPECTED.trim_start_matches("# empty\n");
    if expected == actual {
        return;
    }

    let mut shown = 0usize;
    let mut total = 0usize;
    for (i, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
        if e != a {
            total += 1;
            if shown < 12 {
                shown += 1;
                eprintln!("第 {} 行不同:\n  期望: {e}\n  实际: {a}", i + 1);
            }
        }
    }
    if expected.lines().count() != actual.lines().count() {
        eprintln!(
            "行数不同: 期望 {}, 实际 {}",
            expected.lines().count(),
            actual.lines().count()
        );
    }
    panic!(
        "golden 快照不一致: {total} 行不同。若行为变更是有意的, 用 \
         UPDATE_GOLDEN=1 cargo test --test golden_signals 更新并 review diff。"
    );
}
