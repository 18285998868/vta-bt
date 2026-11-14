//! `app` 的单元测试。

use crate::config;
use crate::engine;
use crate::output;
use crate::strategies::build::build_strategies_checked;

use super::combo::*;
use super::fingerprint::*;
use super::types::*;

/// 构造等长测试列。`KlineColumns` 字段已私有化, 只能经 `new` 构造,
/// 且无法原地改写 —— 需要"内容不同但等长"的列时必须重建。
#[allow(clippy::too_many_arguments)]
fn cols_raw(
    time: Vec<i64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
) -> crate::data::KlineColumns {
    crate::data::KlineColumns::new(time, open, high, low, close, volume).expect("测试列必须等长")
}

/// `n` 根标准 OHLCV 列 (close 逐根 +0.5, 供 `close_of` 覆盖)。
fn cols_with_close(n: usize, close_of: impl Fn(usize) -> f64) -> crate::data::KlineColumns {
    cols_raw(
        (0..n as i64).map(|i| 1_700_000_000 + i * 3600).collect(),
        (0..n).map(|i| 100.0 + i as f64).collect(),
        (0..n).map(|i| 101.0 + i as f64).collect(),
        (0..n).map(|i| 99.0 + i as f64).collect(),
        (0..n).map(close_of).collect(),
        (0..n).map(|i| 10.0 + i as f64).collect(),
    )
}

fn cols(n: usize) -> crate::data::KlineColumns {
    cols_with_close(n, |i| 100.5 + i as f64)
}

/// L3: 缓存指纹必须覆盖**数据内容**。
///
/// 可触发场景: 同一 start~end 窗口先跑一次(只有 1 月数据), 再补进 2 月数据
/// 重跑 —— 旧实现只混入 window 字符串, 两次指纹相同, 于是全部命中旧缓存,
/// 报告仍基于只有 1 月的行情且无任何提示。
#[test]
fn fingerprint_covers_data_identity() {
    let a = cols(50);
    let b = cols(50);
    assert_eq!(
        data_fingerprint(&a),
        data_fingerprint(&b),
        "相同数据必须同指纹(否则缓存永不命中)"
    );

    // 追加 bar(数据被补齐)。`cols` 的取值公式逐根连续, 故 cols(51) 恰等于
    // 在 cols(50) 末尾 push 一根 (i=50: open=150 / high=151 / low=149 / close=150.5)。
    let longer = cols(51);
    assert_ne!(
        data_fingerprint(&a),
        data_fingerprint(&longer),
        "追加 bar 必须改变指纹(否则补数据后仍命中旧缓存)"
    );

    // 同一长度但**价格被改写**(同路径文件原地替换): 只把第 10 根 close +0.01
    let rewritten = cols_with_close(50, |i| {
        if i == 10 {
            100.5 + 10.0 + 0.01
        } else {
            100.5 + i as f64
        }
    });
    assert_ne!(
        data_fingerprint(&a),
        data_fingerprint(&rewritten),
        "同长度但价格变化必须改变指纹(否则行情被替换仍命中旧缓存)"
    );

    // 窗口字符串相同但数据不同 → params_fingerprint 必须不同
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let exec = crate::engine::ExecConfig::default();
    let plain = OutputSet::default();
    let fp_old = params_fingerprint(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "~",
        data_fingerprint(&a),
        &exec,
        plain,
    );
    let fp_new = params_fingerprint(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "~",
        data_fingerprint(&rewritten),
        &exec,
        plain,
    );
    assert_ne!(fp_old, fp_new, "同窗口不同数据必须给出不同缓存文件名");
}

/// 列间必须有分隔符: 否则 `[1,2]+[3]` 与 `[1]+[2,3]` 的字节流相同。
#[test]
fn data_fingerprint_separates_columns() {
    let a = cols_raw(
        vec![1, 2, 3],
        vec![4.0, 5.0, 6.0],
        vec![7.0, 8.0, 9.0],
        vec![10.0, 11.0, 12.0],
        vec![13.0, 14.0, 15.0],
        vec![16.0, 17.0, 18.0],
    );
    // 把 close 的值挪到 open, 长度不变
    let b = cols_raw(
        vec![1, 2, 3],
        vec![13.0, 14.0, 15.0],
        vec![7.0, 8.0, 9.0],
        vec![10.0, 11.0, 12.0],
        vec![4.0, 5.0, 6.0],
        vec![16.0, 17.0, 18.0],
    );
    assert_ne!(
        data_fingerprint(&a),
        data_fingerprint(&b),
        "列内容互换必须改变指纹"
    );
}

/// 执行口径任一变化都必须失效缓存: 前视乐观口径与费率差异会整体改写结果。
#[test]
fn fingerprint_covers_exec_params() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let c = cols(10);
    let fp = data_fingerprint(&c);
    let plain = OutputSet::default();
    let base = params_fingerprint(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &crate::engine::ExecConfig::default(),
        plain,
    );
    for exec in [
        crate::engine::ExecConfig::try_from_bps_fill(2, 0.0, 0.0, false).unwrap(),
        crate::engine::ExecConfig::try_from_bps_fill(1, 2.0, 0.0, false).unwrap(),
        crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 1.0, false).unwrap(),
        crate::engine::ExecConfig::try_from_bps_fill(1, 0.0, 0.0, true).unwrap(),
    ] {
        assert_ne!(
            base,
            params_fingerprint(
                &*s,
                1,
                crate::money::Amount::from_i64(10_000),
                "a~b",
                fp,
                &exec,
                plain
            ),
            "执行口径 {:?} 变化必须失效缓存",
            exec
        );
    }
    assert_ne!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(20_000),
            "a~b",
            fp,
            &crate::engine::ExecConfig::default(),
            plain
        ),
        "资金变化必须失效缓存"
    );
    assert_ne!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~c",
            fp,
            &crate::engine::ExecConfig::default(),
            plain
        ),
        "窗口变化必须失效缓存"
    );
}

/// **回归**: `detail` / `fixed_curves` 开关必须进缓存指纹。
///
/// 可触发场景(真实死循环): 先以 `detail=false` 跑一次 → `temps/` 里落了
/// 结果缓存; 再以 `detail=true` 跑第二次 → 若开关不进指纹, 第二次**全部
/// 命中缓存并直接早退**, 根本走不到 detail 分支 ⇒ `detail/` 目录一个文件
/// 都没有(用户以为拿到了逐笔交易/曲线); 紧接着 `vta --pbo` 报
/// "未找到 *_fixedcurve.parquet, 请先用 detail=true 重跑" —— 而用户**确实**
/// 设了 `detail=true`。死循环。
///
/// 指纹的定义是"这次要产出什么": 产出集不同 ⇒ 身份不同 ⇒ 必然重算。
#[test]
fn fingerprint_covers_output_set_switches() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let fp = data_fingerprint(&cols(10));
    let exec = crate::engine::ExecConfig::default();
    let plain = OutputSet::default();
    let base = params_fingerprint(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        plain,
    );

    // detail 开关变化 ⇒ 指纹变化(否则第二次跑拿不到 detail 产物)。
    assert_ne!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            OutputSet {
                detail: true,
                fixed_curves: false
            }
        ),
        "detail 开关变化必须失效缓存, 否则 detail 产物永远不会被写出"
    );
    // fixed_curves 开关变化 ⇒ 指纹变化(--pbo 的输入只由该开关产出)。
    assert_ne!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            OutputSet {
                detail: false,
                fixed_curves: true
            }
        ),
        "fixed_curves 开关变化必须失效缓存, 否则 *_fixedcurve.parquet 不会被写出"
    );
    // 两个开关都变 ⇒ 与基线不同。
    assert_ne!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            OutputSet {
                detail: true,
                fixed_curves: true
            }
        ),
        "detail + fixed_curves 同时变化必须失效缓存"
    );
    // 同样的开关组合 ⇒ 指纹相同(否则同一次运行会自我失效, 缓存永不命中)。
    assert_eq!(
        base,
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            plain
        ),
        "相同开关组合必须给出相同指纹"
    );
    assert_eq!(
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            OutputSet {
                detail: true,
                fixed_curves: true
            }
        ),
        params_fingerprint(
            &*s,
            1,
            crate::money::Amount::from_i64(10_000),
            "a~b",
            fp,
            &exec,
            OutputSet {
                detail: true,
                fixed_curves: true
            }
        ),
        "相同开关组合必须给出相同指纹"
    );
}

/// 止损配置必须进入缓存指纹: 同一策略同参数换 stop 必须重算, 不能命中
/// 旧缓存后把旧止损结果冒充新结果。
#[test]
fn fingerprint_covers_stop_config() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let fp = data_fingerprint(&cols(10));
    let exec = crate::engine::ExecConfig::default();
    let outputs = OutputSet::default();
    let base = params_fingerprint_with_stop(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        &crate::engine::StopConfig::default(),
        &crate::engine::SizingConfig::default(),
    );
    let sl = crate::engine::StopConfig {
        stop_loss_pct: Some(2.0),
        trailing_stop_pct: None,
        ..crate::engine::StopConfig::default()
    };
    let trail = crate::engine::StopConfig {
        stop_loss_pct: None,
        trailing_stop_pct: Some(5.0),
        ..crate::engine::StopConfig::default()
    };
    let atr = crate::engine::StopConfig {
        atr: Some(crate::engine::AtrStopConfig {
            period: 14,
            mult: 2.0,
            trailing: true,
        }),
        ..crate::engine::StopConfig::default()
    };
    let breakeven = crate::engine::StopConfig {
        breakeven: Some(crate::engine::BreakevenConfig {
            trigger_pct: 2.0,
            offset_pct: 0.5,
        }),
        ..crate::engine::StopConfig::default()
    };
    let with_sl = params_fingerprint_with_stop(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        &sl,
        &crate::engine::SizingConfig::default(),
    );
    let with_trail = params_fingerprint_with_stop(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        &trail,
        &crate::engine::SizingConfig::default(),
    );
    let with_atr = params_fingerprint_with_stop(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        &atr,
        &crate::engine::SizingConfig::default(),
    );
    let with_breakeven = params_fingerprint_with_stop(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        &breakeven,
        &crate::engine::SizingConfig::default(),
    );
    assert_ne!(base, with_sl, "固定止损必须改变缓存指纹");
    assert_ne!(base, with_trail, "追踪止损必须改变缓存指纹");
    assert_ne!(base, with_atr, "ATR 止损必须改变缓存指纹");
    assert_ne!(base, with_breakeven, "保本止损必须改变缓存指纹");
    assert_ne!(with_sl, with_trail, "不同止损配置必须可区分");
    assert_ne!(with_atr, with_breakeven, "不同止损配置必须可区分");
}

/// **回归**: 指纹必须含**实现版本**(源码内容哈希)。
///
/// 可触发场景: 指纹只覆盖"输入"(数据/资金/窗口/口径/参数)时, 改完策略代码
/// 重跑会**全部命中旧缓存** —— 报告里是修复前的结果, 而使用者以为拿到了
/// 新结果, 且**连日志都不会提到缓存命中**。这是研究工具里最隐蔽的
/// "结论悄悄过期"失效模式。
#[test]
fn fingerprint_covers_source_hash() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let exec = crate::engine::ExecConfig::default();
    let outputs = OutputSet::default();
    let fp = data_fingerprint(&cols(10));
    let real = params_fingerprint(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
    );
    // 不含实现版本 ⇒ 必然不同。
    let without = params_fingerprint_with_source(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        "",
    );
    assert_ne!(
        real, without,
        "实现版本必须进指纹, 否则改代码后仍命中旧缓存"
    );
    // 换一个实现版本 ⇒ 也必须不同。
    let other = params_fingerprint_with_source(
        &*s,
        1,
        crate::money::Amount::from_i64(10_000),
        "a~b",
        fp,
        &exec,
        outputs,
        "deadbeefdeadbeef",
    );
    assert_ne!(real, other, "实现版本变化必须改变指纹");
    assert_ne!(without, other);
}

/// 源码哈希必须可用(构建期找到了源文件)。
///
/// 若 build.rs 找不到任何源文件, 它会注入哨兵值 `unknown` 并告警 ——
/// 此时"实现版本"这一维**不随代码变化**, 退化为"没有这一维"。
#[test]
fn source_hash_is_available() {
    let h = source_hash();
    assert!(!h.is_empty(), "源码哈希不得为空");
    assert_eq!(h.len(), 16, "源码哈希应是 16 位十六进制: {h}");
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "源码哈希必须是十六进制: {h}"
    );
}

/// **回归**: 指纹必须**跨机器可复现** —— 整数列用小端序编码。
///
/// 可触发场景: 原实现用 `to_ne_bytes()`(本机字节序), 于是同一份数据在
/// x86 与 ARM 上得到**不同**指纹 ⇒ 缓存无法在异构机器间复用, 且与相邻
/// `fnv_bytes_f64` 的小端编码风格不一致。
///
/// 断言方式: 同一列的指纹必须等于"显式小端编码"的结果。
#[test]
fn fingerprint_uses_little_endian_byte_order() {
    let xs: &[i64] = &[1, -1, i64::MIN, i64::MAX, 0x0102_0304_0506_0708];
    let mut expected = FNV_OFFSET;
    for &x in xs {
        expected = fnv_bytes(expected, &x.to_le_bytes());
    }
    let got = fnv_bytes_i64(FNV_OFFSET, xs);
    assert_eq!(
        got,
        fnv_bytes(expected, &[FNV_COL_SEP]),
        "整数列必须按小端序编码(跨机器可复现)"
    );
}

/// L4 + L11: 行数不符 / 文件读失败都不得静默出"完成"。
#[test]
fn merge_reports_failures_instead_of_silently_dropping() {
    let dir = std::env::temp_dir().join(format!("vta-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let rr = RunResult {
        symbol: "BTCUSDT".into(),
        timeframe: "1d".into(),
        strategy: "sma".into(),
        params: "period=20".into(),
        metrics: engine::SummaryMetrics::default(),
        strategy_params: output::strategy_params_value("period=20"),
        strategy_params_json: output::params_to_json("period=20"),
        bars_used: 300,
        status: RunStatus::Ok,
        delay_bars: 1,
        fee_bps: 0.0,
        slippage_bps: 0.0,
        fill_close: false,
        buy_hold_pnl: crate::money::Amount::from_i64(1),
        buy_hold_return: 0.1,
    };
    let good = dir.join("good.parquet");
    let bad = dir.join("bad.parquet");
    let missing = dir.join("missing.parquet");
    output::write_parquet(
        &good,
        std::slice::from_ref(&rr),
        crate::money::Amount::from_i64(10_000),
    )
    .unwrap();
    std::fs::write(&bad, b"not a parquet file").unwrap();

    let out = dir.join("merged.parquet");

    // 全部不可读 → 必须报错, 而不是"合并 0 条"式的成功
    assert!(
        output::merge_parquet_files(
            &[bad.clone(), missing.clone()],
            &out,
            crate::money::Amount::from_i64(10_000),
        )
        .is_err(),
        "全部失败时必须报错"
    );

    // 部分失败 → 报告仍落盘, 但失败必须被**计数**并进入摘要
    let st = output::merge_parquet_files(
        &[good.clone(), bad, missing],
        &out,
        crate::money::Amount::from_i64(10_000),
    )
    .expect("部分失败仍应产出报告");
    assert_eq!(st.total_files, 3);
    assert_eq!(
        st.failed_files, 2,
        "读失败必须被计数(否则报告缺参数却显示完成)"
    );
    assert_eq!(st.merged_rows, 1);
    assert!(out.exists(), "报告仍应落盘");

    // L4: 缓存行数必须等于参数数。截断文件(少 1 行)不能被当成有效缓存。
    let truncated = dir.join("trunc.parquet");
    output::write_parquet(&truncated, &[], crate::money::Amount::from_i64(10_000)).unwrap();
    assert_eq!(
        output::read_parquet(&truncated).unwrap().len(),
        0,
        "构造 0 行缓存作为反例"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// L6: K 线六列必须等长, 否则引擎会**静默截断**少算 bar。
///
/// 可触发场景: 某列短了 → 引擎 `close.len().min(open.len())` 少回测几根,
/// 指标不报错但 bars_used 与预期不符; `time` 短 → `ts(i)` 退化成用 bar 序号
/// 当时间戳, detail 落盘的时间字段变成假时间。
#[test]
fn equal_length_columns_pass_alignment_check() {
    let c = cols(10);
    assert!(validate_columns(&c, "t").is_ok(), "等长必须通过");
    // 空数据不算"长度不一致", 交由 is_empty 判断
    let empty = cols(0);
    assert!(validate_columns(&empty, "t").is_ok());
}

/// 回归: 价格列必须是**有限正数**, 否则整轮指标会被 NaN 污染。
///
/// 可触发场景: 某根 bar 的 close = 0 ⇒ 成交数量 `qty = cash / price` 变 `inf`
/// ⇒ 权益变 NaN ⇒ sharpe / drawdown / pnl **全部**变 NaN。而 NaN 比 0 更危险:
/// 0 会被读成"零波动低风险", NaN 才会暴露; 且下游的排序/取 top-N 会静默丢行。
/// 校验必须发生在**数据加载边界**, 而不是等到出报告才发现。
#[test]
fn non_positive_or_non_finite_prices_are_rejected_at_the_data_boundary() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let c = crate::data::KlineColumns::new(
            vec![1, 2, 3],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
            vec![10.0, 11.0, 12.0],
            vec![13.0, 14.0, bad],
            vec![16.0, 17.0, 18.0],
        )
        .expect("等长列必须可构造");
        let e = validate_columns(&c, "t")
            .expect_err(&format!("close={bad} 必须被拒, 否则整轮指标变 NaN"));
        assert!(
            format!("{e}").contains("close"),
            "错误消息应指明是哪一列: {e}"
        );
    }
}

/// 回归: 合法价格列必须通过校验(不得误杀)。
#[test]
fn valid_prices_pass_the_boundary_check() {
    let c = cols(20);
    assert!(validate_columns(&c, "t").is_ok());
}

/// 列长相等现由 `KlineColumns` 的**构造**保证, 而非本模块事后校验:
/// `new()` 直接返回 `Err`, reader 输出转换也由等长类型承载,
/// 两条路径都不可能产出不等长实例。故"列长不一致"必须在构造点被拒, 测试随之
/// 移到这里 —— 本函数(`validate_columns`)的等长分支退化为不可达的兜底。
#[test]
fn uneven_columns_are_rejected_at_construction() {
    let uneven = crate::data::KlineColumns::new(
        vec![1, 2, 3],
        vec![4.0, 5.0, 6.0],
        vec![7.0, 8.0, 9.0],
        vec![10.0, 11.0],
        vec![13.0, 14.0, 15.0],
        vec![16.0, 17.0, 18.0],
    );
    let e = uneven.expect_err("少一根的列必须在构造时被拒");
    assert!(
        format!("{e}").contains("equal length"),
        "错误消息应说明长度约束: {e}"
    );
    // 注: reader 的输出经 `DataConfig::load` 转换为 vta 的列结构,
    // 同样只能得到等长实例。
}

/// O9 回归: `run_combo` 的 detail / 非 detail 结果必须经**同一**求值入口构造。
///
/// 历史上两条分支各写一份 16 字段字面量, 新列只加进其中一条 → 报告语义随
/// `--detail` 漂移。P1 把"求值一档参数"收敛到 `ParamEval::evaluate`, 本测试用
/// 源码级断言钉死: `run_combo` 不得内联 `RunResult`, 且分块循环必须调用
/// `evaluate`; `evaluate` 内部正常档走 `run_row`、跳过档走 `invalid_row`。
#[test]
fn combo_rows_are_built_by_the_single_param_evaluator() {
    let src = include_str!("combo.rs");
    let combo = src
        .split("fn run_combo(")
        .nth(1)
        .expect("源码中应存在 run_combo");
    let combo = combo.split("\n/// ").next().expect("run_combo 体应可截取");

    assert!(
        !combo.contains("RunResult {"),
        "run_combo 内不得内联构造 RunResult —— 唯一构造点是 ComboCtx::run_row; \
         内联字面量会让 detail / 非 detail 两条分支的字段集合漂移(O9)"
    );
    assert_eq!(
        combo.matches("eval.evaluate(").count(),
        1,
        "分块循环必须经 ParamEval::evaluate 求值每一档参数"
    );

    let eval = src
        .split("fn evaluate(")
        .nth(1)
        .expect("源码中应存在 ParamEval::evaluate");
    let eval = eval.split("\n/// ").next().expect("evaluate 体应可截取");
    assert!(
        eval.contains("self.ctx.run_row("),
        "正常档必须经 run_row 构造报告行"
    );
    assert!(
        eval.contains("self.ctx.invalid_row("),
        "跳过档必须经 invalid_row 构造报告行"
    );
}

/// P1 回归: `data_fingerprint` 必须在**策略层循环之外**计算一次。
///
/// 可触发场景: 该函数遍历全部 6 列逐字节 FNV(自述~20ms/1M bars)。它原先在
/// `run_combo` 内被调用, 而 `run_combo` 按策略逐个调用 ⇒ 同一份行情被哈希
/// **N 次**(100+ 策略即 100 倍纯浪费), 且随网格规模线性放大。
/// 本测试锁定: `run()` 在策略循环外算 `data_fp` 并以参数传入。
#[test]
fn data_fingerprint_is_hoisted_out_of_the_strategy_loop() {
    let src = concat!(include_str!("run.rs"), include_str!("combo.rs"));
    let run_body = src.split("pub fn run(").nth(1).expect("源码中应存在 run()");
    let run_body = run_body.split("\n#[cfg(test)]").next().expect("run() 体");

    // run() 中恰有一次 data_fingerprint 调用, 且位于策略循环之前。
    assert_eq!(
        run_body.matches("data_fingerprint(").count(),
        1,
        "run() 中应只算一次 data_fingerprint"
    );
    let fp_at = run_body.find("data_fingerprint(").expect("已定位");
    let loop_at = run_body.find("for job in &jobs").expect("已定位策略循环");
    assert!(
        fp_at < loop_at,
        "data_fingerprint 必须在 `for job in &jobs` 之前计算"
    );

    // run_combo 内部不得再算(否则按策略重复)。
    let combo = src
        .split("fn run_combo(")
        .nth(1)
        .expect("源码中应存在 run_combo");
    let combo = combo.split("\n/// ").next().expect("run_combo 体");
    assert!(
        !combo.contains("data_fingerprint("),
        "run_combo 内不得重算 data_fingerprint —— 它由调用方按 (symbol, tf) 对传入"
    );
}

/// 回归: `build_jobs_checked` 的唯一性自检必须在**生产路径** `run()` 上生效。
///
/// 可触发场景: `run()` 曾直接调 `build_strategies`(不做自检), 而
/// `build_strategies_checked` 全仓库无调用方 —— 于是 `param_str` 碰撞完全静默,
/// parquet 的 `strategy_params` 列排序错乱且无任何提示; 文档却宣称该自检
/// "每次启动时"运行。此测试锁定 `run()` 必须走 checked 变体, 且保留 stop。
#[test]
fn run_uses_the_uniqueness_checked_strategy_builder() {
    let src = include_str!("run.rs");
    let body = src.split("pub fn run(").nth(1).expect("源码中应存在 run()");
    let body = body.split("\n#[cfg(test)]").next().expect("run() 函数体");
    assert!(
        body.contains("build_jobs_checked("),
        "run() 必须调用 build_jobs_checked, 否则参数网格碰撞检查与 stop 都会失效"
    );
    assert!(
        !body.contains("= build_strategies("),
        "run() 不得调用未做唯一性自检的 build_strategies"
    );
}

/// 回归: IO 层不得用 `to_str().expect(..)` 把 `Path` 降级成 `&str`。
///
/// 可触发场景: 旧签名收 `&str`, 调用方被迫 `path.to_str().expect("non-UTF-8 path")`。
/// 输出目录来自配置, 用户一旦填入非 UTF-8 路径, 整轮回测在**第一个组合**就 panic,
/// 而不是报错。现签名直接收 `&Path`, 该转换连同 panic 一起消失。
#[test]
fn parquet_io_apis_take_path_and_never_panic_on_non_utf8() {
    let src = concat!(
        include_str!("../output/parquet/report.rs"),
        include_str!("../output/parquet/read.rs"),
        include_str!("../output/parquet/detail.rs"),
    );
    let io_fns = [
        "pub fn write_parquet(",
        "pub fn read_parquet(path: &Path",
        "pub fn read_equity_curves(dir: &Path",
        "pub fn read_fixed_curves(dir: &Path",
    ];
    for sig in io_fns {
        assert!(src.contains(sig), "IO 函数签名应收 &Path: {sig}");
    }
    // `write_parquet` 的签名是跨行的, 单独断言它的第一个参数是 `&Path`。
    assert!(
        src.contains("path: &Path,"),
        "write_parquet 的路径参数必须是 &Path"
    );
    assert!(
        !src.contains("non-UTF-8 path"),
        "生产代码不得用 expect(\"non-UTF-8 path\") 把 Path 降级为 &str"
    );
}

/// O9: `RunResult` 的构造必须**单一来源**, 否则两条分支的字段会漂移。
/// **回归**: 不可计算的档位必须被记成 `RunStatus::Invalid`,
/// 且 `final_equity = balance`(该档未开仓, 权益就是初始资金)——
/// 若留 0, `final_equity` 会与 `total_return = 0` 自相矛盾。
#[test]
fn invalid_row_is_marked_invalid_and_keeps_equity_consistent() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let ctx = ComboCtx {
        balance: crate::money::Amount::from_i64(10_000),
        detail: false,
        fixed_curves: false,
        overwrite: false,
        no_cache: false,
        temps_dir: std::path::PathBuf::from("/tmp"),
        detail_dir: std::path::PathBuf::from("/tmp"),
        window: "~".to_string(),
        exec: crate::engine::ExecConfig::default(),
    };
    let r = ctx.invalid_row(
        "BTCUSDT",
        "1d",
        &*s,
        "period=20",
        300,
        crate::money::Amount::from_i64(1),
        0.1,
    );
    assert_eq!(r.status, RunStatus::Invalid);
    assert_eq!(r.status.as_str(), "invalid");
    assert_eq!(
        r.metrics.final_equity,
        crate::money::Amount::from_i64(10_000),
        "未开仓 ⇒ 权益就是初始资金, 不得为 0"
    );
    assert_eq!(r.metrics.total_return, 0.0);
    // 执行口径必须与正常行同源: invalid 行也是"本次运行"的一部分,
    // 旧实现只填外层 RunResult, 内层 SummaryMetrics 的 delay 留 0。
    assert_eq!(r.metrics.delay_bars, r.delay_bars);
    assert_eq!(r.metrics.fee_bps, r.fee_bps);
    assert_eq!(r.metrics.slippage_bps, r.slippage_bps);
    assert_ne!(r.metrics.delay_bars, 0, "delay_bars 恒 >= 1");
    assert_eq!(r.bars_used, 300, "数据规模不因该档不可算而改变");
    assert_eq!(
        r.buy_hold_pnl,
        crate::money::Amount::from_i64(1),
        "基准不因该档不可算而改变"
    );
}

/// 五个 `status` 取值的落盘文本必须互不相同且稳定。
#[test]
fn run_status_texts_are_distinct() {
    let all = [
        RunStatus::Ok,
        RunStatus::NoSignals,
        RunStatus::NoTrades,
        RunStatus::Invalid,
        RunStatus::Bankrupt,
    ];
    let mut seen = std::collections::HashSet::new();
    for s in all {
        assert!(seen.insert(s.as_str()), "status 文本重复: {}", s.as_str());
    }
    assert_eq!(RunStatus::Ok.as_str(), "ok");
    assert_eq!(RunStatus::NoSignals.as_str(), "no_signals");
    assert_eq!(RunStatus::NoTrades.as_str(), "no_trades");
    assert_eq!(RunStatus::Invalid.as_str(), "invalid");
    assert_eq!(RunStatus::Bankrupt.as_str(), "bankrupt");
    // 落盘文本必须能无损回读; 未知文本必须拒绝, 不得静默当 ok。
    for s in all {
        assert_eq!(RunStatus::parse(s.as_str()), Some(s), "{}", s.as_str());
    }
    assert_eq!(RunStatus::parse("bogus"), None);
    assert_eq!(RunStatus::parse(""), None);
}

/// `Bankrupt` 必须优先于 `Ok`: 跑出过成交但权益归零, 仍不能当正常结果。
#[test]
fn classify_marks_wiped_out_accounts_as_bankrupt() {
    let signals = [crate::strategies::Signal::Buy];
    let healthy = engine::SummaryMetrics {
        num_trades: 1,
        final_equity: crate::money::Amount::from_i64(100),
        ..engine::SummaryMetrics::default()
    };
    assert_eq!(RunStatus::classify(&signals, &healthy), RunStatus::Ok);

    let wiped = engine::SummaryMetrics {
        num_trades: 1,
        final_equity: crate::money::Amount::ZERO,
        ..engine::SummaryMetrics::default()
    };
    assert_eq!(RunStatus::classify(&signals, &wiped), RunStatus::Bankrupt);

    let negative = engine::SummaryMetrics {
        num_trades: 3,
        final_equity: crate::money::Amount::from_i64(-5),
        ..engine::SummaryMetrics::default()
    };
    assert_eq!(
        RunStatus::classify(&signals, &negative),
        RunStatus::Bankrupt
    );

    // 没产生过成交(本金仍为正) 时不能误判为破产。
    let no_trades = engine::SummaryMetrics {
        num_trades: 0,
        final_equity: crate::money::Amount::from_i64(10_000),
        ..engine::SummaryMetrics::default()
    };
    assert_eq!(
        RunStatus::classify(&signals, &no_trades),
        RunStatus::NoTrades
    );
    assert_eq!(
        RunStatus::classify(&[crate::strategies::Signal::Hold], &no_trades),
        RunStatus::NoSignals
    );
}

#[test]
fn run_row_is_the_single_construction_point() {
    let s = build_strategies_checked(&[config::StrategyConfig::Sma(config::MaGridCfg {
        period: config::PeriodParam::Single(20),
    })])
    .expect("sma 网格合法")
    .pop()
    .expect("sma");
    let ctx = ComboCtx {
        balance: crate::money::Amount::from_i64(10_000),
        detail: false,
        fixed_curves: false,
        overwrite: true,
        no_cache: false,
        temps_dir: "t".into(),
        detail_dir: "d".into(),
        window: "a~b".into(),
        exec: engine::ExecConfig::try_from_bps_fill(1, 2.0, 1.0, true).unwrap(),
    };
    let m = engine::SummaryMetrics {
        sharpe_ratio: 1.25,
        ..engine::SummaryMetrics::default()
    };
    let r = ctx.run_row(
        "BTCUSDT",
        "1d",
        &*s,
        "period=20",
        m,
        300,
        RunStatus::NoSignals,
        crate::money::Amount::from_i64(123),
        0.0123,
    );
    // 执行口径字段由 ctx 提供, 不可能与分支漂移
    assert_eq!(r.delay_bars, 1);
    assert!((r.fee_bps - 2.0).abs() < 1e-12);
    assert!((r.slippage_bps - 1.0).abs() < 1e-12);
    assert!(r.fill_close);
    // 内层指标里的执行口径也必须被回填(此前只有 simulate 负责填,
    // 经 run_row 构造的 invalid 行会留下 0)。
    assert_eq!(r.metrics.delay_bars, r.delay_bars);
    assert!((r.metrics.fee_bps - r.fee_bps).abs() < 1e-12);
    assert!((r.metrics.slippage_bps - r.slippage_bps).abs() < 1e-12);
    // params 派生的两列也在此处统一
    assert_eq!(
        r.strategy_params,
        output::strategy_params_value("period=20")
    );
    assert_eq!(r.strategy_params_json, "{\"period\":\"20\"}");
    // L5 两列
    assert_eq!(r.bars_used, 300);
    assert_eq!(r.status, RunStatus::NoSignals);
    assert_eq!(r.metrics.sharpe_ratio, 1.25);
    assert!((r.buy_hold_pnl.to_f64() - 123.0).abs() < 1e-12);
}
