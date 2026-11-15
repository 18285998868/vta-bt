//! 端到端回归: 真实配置里出现的每个策略都必须构建出**非空**的实例。
//!
//! `build_one` 的穷尽性由 `registry.rs` 的单张注册表结构性保证: 枚举变体与
//! 构造臂由同一次宏展开生成, 新增变体不可能"只有枚举、没有构造"。
//! 旧实现靠解析 `build.rs` 源码数匹配臂来兜底, 注册表落地后该检查已冗余。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// 端到端: 真实配置里出现的每个策略都必须构建出**非空**的实例。
#[test]
fn real_configs_build_all_their_strategies() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "config/00.example.toml",
        "config/30.sma-100.toml",
        "config/31.ma-all-1h.toml",
    ] {
        let text = std::fs::read_to_string(root.join(name)).expect("配置应可读");
        let cfg: vta::config::Config = toml::from_str(&text).expect("配置应可解析");
        let jobs = vta_strategies::build::build_jobs_checked(&cfg.strategy)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            jobs.len(),
            cfg.strategy.len(),
            "{name}: 配置了 {} 个策略却只构建出 {} 个 —— 有变体被 build_one 跳过",
            cfg.strategy.len(),
            jobs.len()
        );
    }
}
