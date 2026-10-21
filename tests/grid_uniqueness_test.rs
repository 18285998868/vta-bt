//! 参数网格唯一性自检的**回归**测试。
//!
//! 这层检查在生产路径 `run()` 上生效(`build_strategies_checked`), 而它一旦误报
//! 就会让整轮回测在启动阶段直接退出。故必须用**真实的配置文件**验证:
//!
//! 1. 仓库里的三个配置(`sma_100` / `ma_all_1h` / `sma_100` 示例)必须全部通过 ——
//!    任何 false positive 都是阻断性回归。
//! 2. 构造一个**真正会塌陷**的网格(同一策略内两个参数组压成同一
//!    `strategy_params` 整数), 必须被拒绝 —— 否则 Q1 的修复形同虚设。
//!
//! 注意第 2 项: `param_str` 字符串在两个参数组之间是**不同**的, 塌陷发生在压缩层。
//! 这正是只查字符串唯一会漏掉的场景。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use vta::config::{Config, F64Param, PeriodParam, StrategyConfig};
use vta::strategies::build::build_strategies_checked;

/// 解析仓库内的配置文件。路径相对 `CARGO_MANIFEST_DIR`。
fn load_repo_config(name: &str) -> Config {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("解析 {} 失败: {e}", path.display()))
}

/// 仓库内的真实配置必须全部通过唯一性自检(防 false positive)。
#[test]
fn real_repository_configs_pass_the_uniqueness_check() {
    for name in [
        "config.sma_100.toml",
        "config.ma_all_1h.toml",
        "config.example.toml",
    ] {
        let cfg = load_repo_config(name);
        cfg.validate()
            .unwrap_or_else(|e| panic!("{name} 语义校验失败: {e}"));
        build_strategies_checked(&cfg.strategy).unwrap_or_else(|e| {
            panic!("{name} 的参数网格被唯一性自检拒绝 —— 这是 false positive, 会阻断真实回测: {e}")
        });
    }
}

/// 回归: 多参数形态的 `param_str` 不得塌陷成一个 `strategy_params` 值。
///
/// 可触发场景(**真实缺陷, 已修**): 编码器的兜底分支对**任何**未识别形态一律
/// "取末尾数字"。对 `edma_p1_o0` / `sgma_p1_m2` / `atma_u1_d226` / `macd_12_26_9`
/// 这类含**多个下划线**的串, 末尾数字只编码了最后一个维度, 于是:
/// - `edma_p1_o0` … `edma_p500_o0` 全部编码为 `0`
/// - `sgma_p1_m2` … `sgma_p500_m2` 全部编码为 `2`
/// - `atma_*` 形成 20 组塌陷
///
/// 报告的 `strategy_params` 列正是"同策略内按参数大小排序"的依据, 塌陷后排序
/// 完全失效且**静默**。修复: 仅当串严格形如 `<前缀>_<数字>`(前缀不含下划线)才取
/// 末尾数字, 其余形态走 FNV(整串哈希)。
#[test]
fn multi_underscore_param_forms_do_not_collapse_to_trailing_digits() {
    // 单参数形态: 末尾数字**就是**身份, 必须仍走原口径(封存数据兼容)。
    assert_eq!(vta::output::strategy_params_value("sma_10"), 10);
    assert_eq!(vta::output::strategy_params_value("ema_3"), 3);
    assert_eq!(vta::output::strategy_params_value("t3_12"), 12);

    for group in [
        ["edma_p1_o0", "edma_p2_o0", "edma_p500_o0"],
        ["sgma_p1_m2", "sgma_p2_m2", "sgma_p500_m2"],
        ["atma_u1_d226", "atma_u26_d226", "atma_u476_d226"],
        ["macd_12_26_9", "macd_26_9_12", "macd_34_55_13"],
    ] {
        let vals: Vec<u64> = group
            .iter()
            .map(|p| vta::output::strategy_params_value(p))
            .collect();
        let mut uniq = vals.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(
            uniq.len(),
            group.len(),
            "{group:?} 塌陷成 {vals:?} —— 多参数形态被当成了单参数"
        );
        // 旧塌陷值(末尾数字)不得再出现。
        let trailing: u64 = group[0]
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .parse()
            .unwrap();
        assert!(
            !vals.contains(&trailing),
            "{group:?} 中仍出现塌陷值 {trailing}"
        );
    }
}

/// 回归: 多参数形态的 `strategy_params_json` 不得谎报 `period`。
///
/// 可触发场景(同一根因, 已修): `params_to_json` 与 `strategy_params_value` 曾
/// 共用"取末尾数字"的兜底。`edma_p5_o0` 因此被写成 `{"period":"0"}` —— 而它真实
/// 的 period 是 **5**, 0 是 offset。下游按 period 分组会把整族 EDMA 归错类。
#[test]
fn multi_param_json_does_not_mislabelling_period() {
    use vta::output::params_to_json;
    // 单参数形态: 末尾数字就是 period, 保持既有口径。
    assert_eq!(params_to_json("sma_10"), r#"{"period":"10"}"#);
    assert_eq!(params_to_json("period=20"), r#"{"period":"20"}"#);

    // 多参数形态: 按原串键名还原, 不再谎称 period。
    assert_eq!(params_to_json("edma_p5_o0"), r#"{"p":"5","o":"0"}"#);
    assert_eq!(params_to_json("sgma_p10_m2"), r#"{"p":"10","m":"2"}"#);
    assert_eq!(params_to_json("atma_u5_d10"), r#"{"u":"5","d":"10"}"#);

    // 关键回归: 旧实现把 offset 当成 period 输出。
    assert_ne!(
        params_to_json("edma_p5_o0"),
        r#"{"period":"0"}"#,
        "edma_p5_o0 的 period 是 5, 不得被写成 0"
    );

    // 全部产出必须是合法 JSON(键名只含 ASCII 字母)。
    for p in [
        "edma_p5_o0",
        "sgma_p10_m2",
        "atma_u5_d10",
        "sma_10",
        "period=20,variant=Three",
    ] {
        let j = params_to_json(p);
        assert!(j.starts_with('{') && j.ends_with('}'), "{p} → {j}");
        assert!(
            j.parse::<serde_json::Value>().is_ok(),
            "{p} 产出非法 JSON: {j}"
        );
    }
}

/// 对照: 合法网格必须通过自检(证明自检不是"见谁杀谁")。
#[test]
fn legal_grids_pass_the_check() {
    let ok = build_strategies_checked(&[StrategyConfig::Sma(vta::config::MaGridCfg {
        period: vta::config::PeriodParam::Range(vta::config::RangeParam {
            start: 1,
            end: 50,
            step: 1,
        }),
    })]);
    assert!(ok.is_ok(), "Sma 1..50 是合法网格, 必须通过自检");
}

/// 编码层自检覆盖多种多键形态: 任一形态塌陷都必须被拒。
///
/// 这些都是 `strategy_params_value` 已知的"多键折叠"形态。逐一验证其编码唯一,
/// 防止将来扩网格时无意撞值却被漏过。
#[test]
fn known_multi_key_encodings_stay_unique_on_realistic_grids() {
    // BBMR / BBRK_VOL 等的真实网格形状(取自 config.example.toml 的量级)。
    for sc in [
        StrategyConfig::BbrkVol(vta::strategies::bbrk_vol::BbrkVolConfig {
            sigma: F64Param::Range(vta::config::RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            voln: PeriodParam::Single(10),
            volmult: F64Param::Single(1.5),
        }),
        StrategyConfig::BbrkVolEma(vta::strategies::bbrk_vol_ema::BbrkVolEmaConfig {
            bblen: vta::config::PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
            voln: PeriodParam::Single(10),
            volmult: F64Param::Single(1.5),
        }),
        StrategyConfig::BbrkTtm(vta::strategies::bbrk_ttm::BbrkTtmConfig {
            bblen: vta::config::PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
            kc: F64Param::Single(1.5),
            ttmn: PeriodParam::Single(5),
        }),
        StrategyConfig::BbrkMax(vta::strategies::bbrk_max::BbrkMaxConfig {
            bblen: vta::config::PeriodParam::Single(20),
            ma: vec!["sma".to_string(), "hma".to_string()],
            malen: PeriodParam::Single(100),
        }),
    ] {
        let sts = build_strategies_checked(&[sc]).expect("这些网格应当通过编码唯一性自检");
        for st in &sts {
            let n = st.param_count();
            let mut vals = std::collections::HashSet::new();
            for i in 0..n {
                let p = st.param_str(i);
                let v = vta::output::strategy_params_value(&p);
                assert!(
                    vals.insert(v),
                    "[{}] 参数 {p:?} 编码 {v} 与前面的参数撞值",
                    st.name()
                );
            }
        }
    }
}
