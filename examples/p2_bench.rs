//! `p2_bench`: 实测 `params_fingerprint` 里的 `param_str` 分配成本占比。
//!
//! 用法: `cargo run --release --example p2_bench`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::time::Instant;
use vta::config::Config;
use vta::strategies::build::build_strategies_checked;

fn bench() {
    // 仓库根目录: 优先取环境变量, 否则用编译期已知的 `CARGO_MANIFEST_DIR`。
    // 硬编码绝对路径会让 example 换台机器就失效。
    let root: std::path::PathBuf = match std::env::var("VTA_REPO_ROOT") {
        Ok(dir) => dir.into(),
        Err(_) => env!("CARGO_MANIFEST_DIR").into(),
    };
    let text = std::fs::read_to_string(root.join("config.ma_all_1h.toml")).unwrap();
    let cfg: Config = toml::from_str(&text).unwrap();
    // 用**带唯一性自检**的构建器: 与生产路径 `app::run` 同源,
    // 免得 benchmark 测的是一个生产永远不会跑的网格。
    let sts = build_strategies_checked(&cfg.strategy).unwrap();

    let total_params: usize = sts.iter().map(|s| s.param_count()).sum();
    println!("策略数={} 参数总档数={}", sts.len(), total_params);

    // 预热
    for st in &sts {
        for i in 0..st.param_count() {
            std::hint::black_box(st.param_str(i));
        }
    }

    // A) param_str 全量调用一次(= params_fingerprint 的成本 + 行构造的一半)
    let t = Instant::now();
    let mut acc = 0usize;
    for st in &sts {
        for i in 0..st.param_count() {
            acc += std::hint::black_box(st.param_str(i)).len();
        }
    }
    let a = t.elapsed();
    println!("A) param_str 全量调用: {:?}  (acc={acc})", a);

    // B) 缓存后复用(只算一次 + 读长度)
    let t = Instant::now();
    let mut acc2 = 0usize;
    for st in &sts {
        let cached: Vec<String> = (0..st.param_count()).map(|i| st.param_str(i)).collect();
        for p in &cached {
            acc2 += std::hint::black_box(p).len();
        }
    }
    let b = t.elapsed();
    println!("B) 物化一次 + 读:     {:?}  (acc={acc2})", b);

    // C) 引擎 simulate 的成本(用真实长度的价格序列)
    let n = 60_000usize;
    let close: Vec<f64> = (0..n)
        .map(|i| 100.0 + (i as f64 * 0.01).sin() * 20.0)
        .collect();
    let sig = vec![vta::strategies::Signal::Hold; n];
    let exec = vta::engine::ExecConfig::default();
    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(vta::engine::simulate(
            &close, &close, &sig, 10_000.0, 365.0, &exec,
        ));
    }
    let c = t.elapsed() / 20;
    println!("C) 单次 simulate({n} bars): {:?}", c);

    println!();
    println!("=== 结论 ===");
    println!(
        "param_str(A) ≈ simulate(C) 的 {:.1}%",
        a.as_secs_f64() / c.as_secs_f64() * 100.0
    );
    println!(
        "若一个组合要跑 C 次 simulate, param_str(A) 占该组合的 {:.2}%",
        a.as_secs_f64() / (c.as_secs_f64() * 20.0) * 100.0
    );
}

fn main() {
    bench();
}
