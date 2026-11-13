//! 文档与代码的一致性测试。
//!
//! README / `docs/` 里的 TOML 代码块是使用者唯一的手册。此前 README 写着
//! `timeframes = [...]`(复数), 而 `DataSection` 的字段是 `timeframe`(单数) ——
//! 于是**文档自带的冒烟配置根本无法解析**, 使用者照抄必然报错。
//!
//! 本测试把文档里每个 `toml` 代码块都真跑一遍 `Config` 反序列化 + `validate`，
//! 使文档腐化在 `cargo test` 阶段就暴露, 而不是等到使用者踩坑。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use vta::config::Config;

/// 所有参与一致性检查的文档：(路径, 内容)。
const DOCS: &[(&str, &str)] = &[
    ("README.md", include_str!("../README.md")),
    (
        "docs/architecture.md",
        include_str!("../docs/architecture.md"),
    ),
    (
        "docs/configuration.md",
        include_str!("../docs/configuration.md"),
    ),
    ("docs/invariants.md", include_str!("../docs/invariants.md")),
    ("docs/pbo.md", include_str!("../docs/pbo.md")),
    ("docs/reviews.md", include_str!("../docs/reviews.md")),
];

/// 取出文档中所有 ```toml 代码块。
fn toml_blocks(md: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = md;
    while let Some(start) = rest.find("```toml\n") {
        rest = &rest[start + "```toml\n".len()..];
        match rest.find("```") {
            Some(end) => {
                blocks.push(rest[..end].to_string());
                rest = &rest[end..];
            }
            None => break,
        }
    }
    blocks
}

#[test]
fn docs_have_toml_examples() {
    let blocks = toml_blocks(DOCS[0].1);
    assert!(!blocks.is_empty(), "README 应至少包含一个 toml 代码块");
}

/// 文档里的每个 TOML 块都必须能被真实解析并通过语义校验。
#[test]
fn docs_toml_blocks_are_valid_config() {
    let mut total = 0usize;
    for (path, md) in DOCS {
        for (i, block) in toml_blocks(md).iter().enumerate() {
            total += 1;
            let cfg: Config = toml::from_str(block).unwrap_or_else(|e| {
                panic!("{path} 第 {} 个 toml 块无法解析: {e}\n---\n{block}", i + 1)
            });
            cfg.validate()
                .unwrap_or_else(|e| panic!("{path} 第 {} 个 toml 块语义校验失败: {e}", i + 1));
        }
    }
    assert!(total > 0, "文档未找到任何 toml 代码块");
}

/// 文档声明的字段名必须与 `DataSection` 的真实字段一致(防止再次写成复数)。
#[test]
fn docs_use_the_real_field_names() {
    for (path, md) in DOCS {
        // `deny_unknown_fields` 之下, 任何拼错的键都会直接报错 —— 这里做静态提醒。
        assert!(
            !md.contains("timeframes = ["),
            "{path} 里 `timeframes` 是错的: 字段名为 `timeframe`(单数), 支持单值或数组"
        );
        assert!(
            !md.contains("默认 0/open/0/0"),
            "{path} 里 `delay_bars` 默认值写错了: 默认 **1** 且 0 会被拒绝(前视乐观口径)"
        );
    }
}

/// 关键设计约束的声明条数与实际小节数一致(避免新增条目忘记改标题计数)。
#[test]
fn invariants_count_matches_sections() {
    let md = include_str!("../docs/invariants.md");
    let section = md
        .split("# 关键设计约束")
        .nth(1)
        .expect("docs/invariants.md 应有『关键设计约束』章节");
    let header = section.lines().next().unwrap_or_default();
    let declared: usize = header
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    let actual = section.lines().filter(|l| l.starts_with("### ")).count();
    assert_eq!(
        declared, actual,
        "docs/invariants.md 标题声明 {declared} 条, 实际有 {actual} 个 `### ` 小节 —— 增删条目后请同步标题"
    );
}
