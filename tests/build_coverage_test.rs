//! P5 回归: `build_one` 必须覆盖 `StrategyConfig` 的**每一个**变体。
//!
//! 可触发场景: 原实现有三处手工同步的变体清单(`build_ma!` 宏的 `unreachable!()`
//! 兜底、`build_strategies` 的 `matches!` 排除表、60 条 `filter_map` 分支)。
//! 在前两处漏改**不会报错**, 只会让该策略被静默跳过 —— 配置里写了却根本不跑,
//! 报告里少了整族参数且无任何提示。
//!
//! 现在 `build_one` 是单个**穷尽 match**(无 `_` 兜底), 编译器强制覆盖。本测试
//! 从源码侧再钉一道, 防止有人日后加回 `_ => None` 兜底。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
/// 源码级: `build_one` 不得有 `_ => None` 兜底, 且必须穷尽。
///
/// 真正的编译期保证来自穷尽 match; 这条测试的价值在于: 当有人为了"少写几个
/// 分支"而加回 `None` 兜底时, 测试会立刻失败并说明后果。
#[test]
fn build_one_has_no_catch_all_arm() {
    let src = include_str!("../src/app/05_strategies/build.rs");
    let body = src.split("fn build_one(").nth(1).expect("build_one 应存在");
    let body = body.split("\n}\n").next().expect("build_one 体应可截取");

    assert!(
        !body.contains("_ => None"),
        "build_one 不得有 `_ => None` 兜底 —— 新增变体会被静默跳过(配置写了却不跑)"
    );
    assert!(
        !body.contains("unreachable!"),
        "build_one 不得用 unreachable! 兜底未接线的变体"
    );
    assert!(
        !body.contains("=> None"),
        "build_one 的每个变体都应产出策略, 不应返回 None"
    );
}

/// 每个变体都接上了构造: `StrategyConfig` 变体数 == `build_one` 的匹配臂数。
#[test]
fn every_variant_has_a_construction_arm() {
    let cfg_src = include_str!("../src/config.rs");
    let enum_body = cfg_src
        .split("pub enum StrategyConfig {")
        .nth(1)
        .expect("StrategyConfig 应存在")
        .split("\n}")
        .next()
        .expect("枚举体应可截取");
    // 变体行形如 `    Sma(MaGridCfg),`
    let variants: Vec<&str> = enum_body
        .lines()
        .map(str::trim)
        .filter(|l| l.ends_with(')') || l.ends_with("),"))
        .filter_map(|l| l.split('(').next())
        .filter(|v| !v.is_empty() && v.chars().next().is_some_and(char::is_uppercase))
        .collect();

    let build_src = include_str!("../src/app/05_strategies/build.rs");
    let body = build_src
        .split("fn build_one(")
        .nth(1)
        .expect("build_one 应存在")
        .split("\n}\n")
        .next()
        .expect("build_one 体应可截取");
    let arms: Vec<&str> = body
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("StrategyConfig::")
                .and_then(|r| r.split('(').next())
        })
        .collect();

    assert_eq!(
        arms.len(),
        variants.len(),
        "StrategyConfig 有 {} 个变体, build_one 只匹配了 {} 个 —— 有变体被静默跳过",
        variants.len(),
        arms.len()
    );
    for v in &variants {
        assert!(
            arms.contains(v),
            "变体 {v} 在 build_one 中没有构造臂 —— 该策略会被静默跳过"
        );
    }
}

/// 端到端: 真实配置里出现的每个策略都必须构建出**非空**的实例。
#[test]
fn real_configs_build_all_their_strategies() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in [
        "config.example.toml",
        "config.sma_100.toml",
        "config.ma_all_1h.toml",
    ] {
        let text = std::fs::read_to_string(root.join(name)).expect("配置应可读");
        let cfg: vta::config::Config = toml::from_str(&text).expect("配置应可解析");
        let sts = vta::strategies::build::build_strategies_checked(&cfg.strategy)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            sts.len(),
            cfg.strategy.len(),
            "{name}: 配置了 {} 个策略却只构建出 {} 个 —— 有变体被 build_one 跳过",
            cfg.strategy.len(),
            sts.len()
        );
    }
}
