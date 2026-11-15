//! 参数网格唯一性自检的**回归**测试。
//!
//! 这层检查在生产路径 `run()` 上生效(`build_strategies_checked`), 而它一旦误报
//! 就会让整轮回测在启动阶段直接退出。故必须用**真实的配置文件**验证:
//!
//! 1. 仓库里的配置必须全部通过 —— 任何 false positive 都是阻断性回归。
//! 2. 构造真实的多键网格: 文本与整数编码都不得塌陷。
//!
//! 注意第 2 项: 文本不同**不蕴含**整数不同。`ParamSet` 让两者同源产出, 但定宽
//! 打包越界仍会回退哈希; 唯一性自检必须同时守住两层。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use vta::config::Config;
use vta_strategies::build::{build_jobs_checked, build_strategies_checked};
use vta_strategies::{
    F64Param, ParamSet, PeriodParam, PoleCount, RangeParam, StrategyConfig, bbrk_max, ma_atr,
    ma_slope_atr_gate, mabb_long, mabb_short, macd, sgma, ssf, vwap,
};

/// 解析仓库内的配置文件。路径相对 `CARGO_MANIFEST_DIR`。
fn load_repo_config(name: &str) -> Config {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("解析 {} 失败: {e}", path.display()))
}

fn single(v: usize) -> PeriodParam {
    PeriodParam::Single(v)
}

fn range(start: usize, end: usize) -> PeriodParam {
    PeriodParam::Range(RangeParam {
        start,
        end,
        step: 1,
    })
}

fn f64_range(start: f64, end: f64, step: f64) -> F64Param {
    F64Param::Range(RangeParam { start, end, step })
}

/// 仓库内的真实配置必须全部通过唯一性自检(防 false positive)。
#[test]
fn real_repository_configs_pass_the_uniqueness_check() {
    for name in [
        "config/30.sma-100.toml",
        "config/31.ma-all-1h.toml",
        "config/00.example.toml",
    ] {
        let cfg = load_repo_config(name);
        cfg.validate()
            .unwrap_or_else(|e| panic!("{name} 语义校验失败: {e}"));
        build_jobs_checked(&cfg.strategy).unwrap_or_else(|e| {
            panic!("{name} 的参数网格被唯一性自检拒绝 —— 这是 false positive, 会阻断真实回测: {e}")
        });
    }
}

/// 对照: 合法网格必须通过自检(证明自检不是"见谁杀谁")。
#[test]
fn legal_grids_pass_the_check() {
    let ok = build_strategies_checked(&[StrategyConfig::Sma(vta_strategies::MaGridCfg {
        period: vta_strategies::PeriodParam::Range(vta_strategies::RangeParam {
            start: 1,
            end: 50,
            step: 1,
        }),
    })]);
    assert!(ok.is_ok(), "Sma 1..50 是合法网格, 必须通过自检");
}

/// 收集单条配置的全部参数组, 并断言文本/整数编码两两不同、JSON 合法。
fn unique_params(config: StrategyConfig, label: &str) -> Vec<ParamSet> {
    let sts = build_strategies_checked(&[config])
        .unwrap_or_else(|e| panic!("{label}: 参数网格被唯一性自检拒绝: {e}"));
    let mut out = Vec::new();
    for st in &sts {
        let mut texts = std::collections::HashSet::new();
        let mut codes = std::collections::HashSet::new();
        for i in 0..st.param_count() {
            let p = st.param_set(i);
            assert!(texts.insert(p.text().to_string()), "{label}: 文本重复");
            assert!(codes.insert(p.code()), "{label}: 整数编码重复");
            assert!(
                p.json().parse::<serde_json::Value>().is_ok(),
                "{label}: 非法 JSON {}",
                p.json()
            );
            out.push(p);
        }
    }
    out
}

fn json_keys(p: &ParamSet) -> Vec<&str> {
    p.pairs().iter().map(|(k, _)| k.as_str()).collect()
}

/// C3 回归: 定宽打包族必须把**每个声明维度**都编进整数, 且 JSON 键名诚实。
///
/// 旧实现要么整串退回 FNV-32, 要么只编码前两维 —— 文本不同、整数相同,
/// 报告的 `strategy_params` 排序会静默错乱。
#[test]
fn c3_encoding_covers_every_declared_dimension() {
    let macd_params = unique_params(
        StrategyConfig::Macd(macd::MacdConfig {
            groups: vec![[12, 26, 9], [26, 9, 12], [34, 55, 13]],
        }),
        "macd",
    );
    assert_eq!(
        json_keys(&macd_params[0]),
        ["fast", "slow", "signal"],
        "macd JSON 不得把 fast 误标成 period"
    );

    let sgma_params = unique_params(
        StrategyConfig::Sgma(sgma::SgmaConfig {
            period: range(10, 20),
            poly_order: Some(PeriodParam::Range(RangeParam {
                start: 1,
                end: 2,
                step: 1,
            })),
        }),
        "sgma",
    );
    assert_eq!(json_keys(&sgma_params[0]), ["p", "m"]);

    let vwap_params = unique_params(
        StrategyConfig::Vwap(vwap::VwapConfig {
            thresh: f64_range(10.0, 30.0, 10.0),
        }),
        "vwap",
    );
    assert_eq!(vwap_params.len(), 3);
    assert_eq!(json_keys(&vwap_params[0]), ["thresh"]);

    let fiali_params = unique_params(
        StrategyConfig::Fiali(vta_strategies::fiali::FialiConfig {}),
        "fiali",
    );
    assert_eq!(fiali_params.len(), 1);
    assert_eq!(fiali_params[0].text(), "base");
    assert_eq!(fiali_params[0].code(), 0);
    assert_eq!(fiali_params[0].json(), "{}");

    let gate_params = unique_params(
        StrategyConfig::MaSlopeAtrGate(ma_slope_atr_gate::MaSlopeAtrGateConfig {
            ma: single(100),
            slope_k: single(20),
            atr_n: single(14),
            atr_kind: ma_slope_atr_gate::AtrKind::Sma,
            z_win: single(60),
            th: f64_range(-1.0, 1.0, 1.0),
        }),
        "ma_slope_atr_gate",
    );
    assert_eq!(
        json_keys(&gate_params[0]),
        ["ma", "k", "atr", "kind", "z", "th"]
    );

    // 维度丢失修复: mult/atrlen 必须各自影响编码。
    let ma_atr_params = unique_params(
        StrategyConfig::MaAtr(ma_atr::MaAtrConfig {
            ma: vec!["sma".to_string()],
            len: single(20),
            mult: f64_range(1.0, 2.0, 1.0),
            atrlen: range(7, 14),
        }),
        "ma_atr",
    );
    // range 的 end 为闭区间: mult 2 档 × atrlen 8 档 = 16。
    assert_eq!(ma_atr_params.len(), 16);
    assert_eq!(
        json_keys(&ma_atr_params[0]),
        ["ma", "len", "mult", "atrlen"]
    );

    let mabb_long_params = unique_params(
        StrategyConfig::MabbLong(mabb_long::MabbLongConfig {
            ma: vec!["sma".to_string()],
            bblen: single(20),
            sigma: f64_range(1.5, 2.5, 1.0),
            stopk: f64_range(0.0, 1.0, 1.0),
            atrlen: range(7, 14),
        }),
        "mabb_long",
    );
    // stopk=0 时 atrlen 去重为单值: sigma 2 × (1 + atrlen 8) = 18。
    assert_eq!(mabb_long_params.len(), 18);
    assert_eq!(
        json_keys(&mabb_long_params[0]),
        ["ma", "len", "sigma", "stopk", "atrlen"]
    );

    let mabb_short_params = unique_params(
        StrategyConfig::MabbShort(mabb_short::MabbShortConfig {
            ma: vec!["sma".to_string()],
            bblen: single(20),
            sigma: f64_range(1.5, 2.5, 1.0),
            stopk: f64_range(0.0, 1.0, 1.0),
            atrlen: range(7, 14),
        }),
        "mabb_short",
    );
    assert_eq!(mabb_short_params.len(), 18);

    let bbrk_max_params = unique_params(
        StrategyConfig::BbrkMax(bbrk_max::BbrkMaxConfig {
            ma: vec!["sma".to_string(), "hma".to_string()],
            malen: single(100),
            bblen: range(20, 22),
        }),
        "bbrk_max",
    );
    assert_eq!(bbrk_max_params.len(), 6, "bblen 必须参与网格");
    assert_eq!(json_keys(&bbrk_max_params[0]), ["ma", "malen", "bblen"]);

    let ssf_params = unique_params(
        StrategyConfig::Ssf(ssf::SsfConfig {
            period: range(10, 20),
            poles: PoleCount::Two,
        }),
        "ssf",
    );
    assert_eq!(json_keys(&ssf_params[0]), ["period", "poles", "variant"]);
}

/// 编码层自检覆盖多种多键形态: 任一形态塌陷都必须被拒。
///
/// 这些都是 `ParamSet::code` 已知的"多键折叠"形态。逐一验证其编码唯一,
/// 防止将来扩网格时无意撞值却被漏过。
#[test]
fn known_multi_key_encodings_stay_unique_on_realistic_grids() {
    // BBRK_VOL 等的真实网格形状(取自 config/00.example.toml 的量级)。
    for sc in [
        StrategyConfig::BbrkVol(vta_strategies::bbrk_vol::BbrkVolConfig {
            sigma: F64Param::Range(vta_strategies::RangeParam {
                start: 1.5,
                end: 2.5,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            voln: PeriodParam::Single(10),
            volmult: F64Param::Single(1.5),
        }),
        StrategyConfig::BbrkVolEma(vta_strategies::bbrk_vol_ema::BbrkVolEmaConfig {
            bblen: vta_strategies::PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
            voln: PeriodParam::Single(10),
            volmult: F64Param::Single(1.5),
        }),
        StrategyConfig::BbrkTtm(vta_strategies::bbrk_ttm::BbrkTtmConfig {
            bblen: vta_strategies::PeriodParam::Single(20),
            sigma: F64Param::Single(2.5),
            kc: F64Param::Single(1.5),
            ttmn: PeriodParam::Single(5),
        }),
        StrategyConfig::BbrkMax(vta_strategies::bbrk_max::BbrkMaxConfig {
            bblen: vta_strategies::PeriodParam::Single(20),
            ma: vec!["sma".to_string(), "hma".to_string()],
            malen: PeriodParam::Single(100),
        }),
    ] {
        let sts = build_strategies_checked(&[sc]).expect("这些网格应当通过编码唯一性自检");
        for st in &sts {
            let mut vals = std::collections::HashSet::new();
            for i in 0..st.param_count() {
                let p = st.param_set(i);
                assert!(
                    vals.insert(p.code()),
                    "[{}] 参数 {:?} 编码 {} 与前面的参数撞值",
                    st.name(),
                    p.text(),
                    p.code()
                );
            }
        }
    }
}
