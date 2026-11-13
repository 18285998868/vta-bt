//! `parquet` 的单元测试。

use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch};
use arrow::datatypes::Field;

use crate::app::RunResult;
use crate::engine::SummaryMetrics;

use super::columns::*;
use super::params::*;
use super::report::*;
use super::schema::*;
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
        u64::from(super::params::fnv1a_32("base")),
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

/// `trade_size` 摘要列必须反映 sizing 后缀, 而不是恒为 100%。
#[test]
fn trade_size_label_reflects_sizing_suffix() {
    assert_eq!(trade_size_label("entry=20,exit=10"), "100%");
    assert_eq!(
        trade_size_label("entry=20,exit=10;size=fixed,size_fraction_pct=20"),
        "20%"
    );
    assert_eq!(
        trade_size_label("entry=20,exit=10;stop_loss_pct=2,size=kelly,size_lookback=50"),
        "kelly"
    );
    assert_eq!(
        trade_size_label("entry=20,exit=10;size=atr,size_risk_pct=1"),
        "atr"
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
    let files = write_detail_parquet(std::slice::from_ref(&d), &detail, true, combo_id).unwrap();
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
            strategy_stop_exits: 2,
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
    assert_eq!(back[0].metrics.strategy_stop_exits, 2);
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
