//! `config` 的单元测试。

use super::backtest::*;
use super::strategy::*;
use super::*;

fn make_toml(balance: f64, symbol: bool, strategy: bool) -> String {
    format!(
        "{}\n[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = {}\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = {balance}\n",
        if strategy {
            "[[strategy]]\ntype = \"sma\"\nperiod = 20"
        } else {
            "strategy = []"
        },
        if symbol { "\"BTCUSDT\"" } else { "\"\"" },
    )
}

#[test]
fn validate_accepts_full_config() {
    let cfg: Config = toml::from_str(&make_toml(10000.0, true, true)).unwrap();
    assert!(cfg.validate().is_ok());
}

/// **契约**: `fixed_curves` 依赖 `detail`。单独开 fixed_curves 必须**配置期**
/// 拒绝, 不得静默不写文件、等到 `--pbo` 才报"未找到曲线"。
#[test]
fn validate_rejects_fixed_curves_without_detail() {
    let toml = format!(
        "{}\n[output]\nfixed_curves = true\n",
        make_toml(10000.0, true, true)
    );
    let cfg: Config = toml::from_str(&toml).expect("配置本身应可解析");
    let e = cfg.validate().expect_err("fixed_curves 单独开启必须被拒");
    assert!(format!("{e}").contains("output.detail = true"), "{e}");

    let ok_toml = format!(
        "{}\n[output]\ndetail = true\nfixed_curves = true\n",
        make_toml(10000.0, true, true)
    );
    let ok: Config = toml::from_str(&ok_toml).expect("配置本身应可解析");
    assert!(ok.validate().is_ok());
}

/// newtype 改造后: 资金/费率/延迟的非法值在**反序列化时**就被拒(自带行列号),
/// 不再依赖 `validate()` 的逐条 `if`。
#[test]
fn illegal_exec_params_rejected_at_parse_time() {
    assert!(toml::from_str::<Config>(&make_toml(0.0, true, true)).is_err());
    assert!(toml::from_str::<Config>(&make_toml(-100.0, true, true)).is_err());
    assert!(toml::from_str::<Config>(&make_toml(f64::INFINITY, true, true)).is_err());
    let zero_delay = make_toml(10000.0, true, true).replace(
        "initial_balance = 10000",
        "initial_balance = 10000.0\ndelay_bars = 0",
    );
    assert!(toml::from_str::<Config>(&zero_delay).is_err());
    for key in ["fee_bps", "slippage_bps"] {
        let neg = make_toml(10000.0, true, true).replace(
            "initial_balance = 10000",
            &format!("initial_balance = 10000.0\n{key} = -1.0"),
        );
        assert!(toml::from_str::<Config>(&neg).is_err(), "{key}=-1 应被拒");
    }
}

/// 类型层保证: 合法值解析出的字段使其不可能表示非法值。
#[test]
fn parsed_exec_params_valid_by_construction() {
    let cfg: Config = toml::from_str(&make_toml(10000.0, true, true)).unwrap();
    assert_eq!(cfg.backtest.initial_balance.get(), 10000.0);
    assert!(cfg.backtest.delay_bars.get() >= 1);
    assert!(cfg.backtest.fee_bps.get() >= 0.0);
    assert!(cfg.backtest.slippage_bps.get() >= 0.0);
    assert!(cfg.validate().is_ok());
}

#[test]
fn missing_delay_bars_defaults_to_one() {
    let cfg: Config = toml::from_str(&make_toml(10000.0, true, true)).unwrap();
    assert_eq!(cfg.backtest.delay_bars.get(), 1);
    assert!(cfg.validate().is_ok());
}

#[test]
fn validate_rejects_empty_symbols() {
    let cfg: Config = toml::from_str(&make_toml(10000.0, false, true)).unwrap();
    assert!(cfg.validate().is_err());
}

/// **回归**: bar 数参数必须是**整数** —— 小数值必须被**拒绝**, 不得静默取整。
///
/// 可触发场景: 这些参数原先用 `F64Param` 承载, 再 `.round() as usize` ——
/// 于是 `bblen = 20.4` 是**合法**配置, 静默变成 20, 两个不同的 TOML 值
/// 映射到同一参数串(唯一性自检报的是"参数串重复"这种让人困惑的错)。
/// 换成 `PeriodParam(usize)` 后, `20.4` 在**反序列化期**即被拒。
#[test]
fn bar_count_params_reject_fractional_values() {
    // 整数必须照常通过。
    let ok: Config = toml::from_str(
        "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\n\
         timeframe = [\"1d\"]\n[backtest]\ninitial_balance = 10000.0\n\
         [[strategy]]\ntype = \"bbrk_don\"\nsigma = 2.5\nbblen = 20\nexitn = 10\n",
    )
    .expect("整数 bar 数必须可解析");
    assert!(ok.validate().is_ok());

    // 小数必须被拒(反序列化期)。
    let bad: Result<Config, _> = toml::from_str(
        "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\n\
         timeframe = [\"1d\"]\n[backtest]\ninitial_balance = 10000.0\n\
         [[strategy]]\ntype = \"bbrk_don\"\nsigma = 2.5\nbblen = 20.4\nexitn = 10\n",
    );
    assert!(bad.is_err(), "bblen = 20.4 必须被拒(不得静默取整成 20)");
}

/// **回归**: `PeriodParam` 的范围展开必须与 `F64Param` 的整数档位一致,
/// 且 `step = 0` 不得死循环。
#[test]
fn period_param_expansion_is_safe() {
    assert_eq!(PeriodParam::Single(20).values(), vec![20]);
    assert_eq!(
        PeriodParam::Range(RangeParam {
            start: 3,
            end: 10,
            step: 7
        })
        .values(),
        vec![3, 10]
    );
    // step = 0: 必须返回空(而不是死循环)。
    assert!(
        PeriodParam::Range(RangeParam {
            start: 3,
            end: 10,
            step: 0
        })
        .values()
        .is_empty()
    );
    // 越界校验。
    let p = PeriodParam::Range(RangeParam {
        start: 1,
        end: 5000,
        step: 1,
    });
    assert!(p.validate_range("x", 2, 2000).is_err());
    assert!(PeriodParam::Single(20).validate_range("x", 2, 2000).is_ok());
}

/// **回归**: 含路径穿越成分的品种名/周期名必须被拒。
///
/// 可触发场景: 品种名**直接进入落盘文件名**(`{symbol}_{tf}_{strategy}_…`),
/// 故 `symbol = "../evil"` 会让产物写到输出目录的**上一层**。
#[test]
fn validate_rejects_path_traversal_in_symbol_and_timeframe() {
    for bad_symbol in ["../evil", "a/b", "a\\b", "..", "a:b", "a*b"] {
        let toml = format!(
            "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = {bad_symbol:?}\n\
             timeframe = [\"1d\"]\n[backtest]\ninitial_balance = 10000.0\n\
             [[strategy]]\ntype = \"sma\"\nperiod = 20\n"
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        let e = cfg.validate().expect_err("含路径穿越成分的品种名必须被拒");
        assert!(
            format!("{e}").contains("data.symbol"),
            "错误消息应指向 data.symbol: {e}"
        );
    }
    for bad_tf in ["../1d", "1d/../x"] {
        let toml = format!(
            "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\n\
             timeframe = [{bad_tf:?}]\n[backtest]\ninitial_balance = 10000.0\n\
             [[strategy]]\ntype = \"sma\"\nperiod = 20\n"
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        assert!(
            cfg.validate().is_err(),
            "含路径穿越成分的周期名必须被拒: {bad_tf}"
        );
    }
    // 真实品种名与周期名必须照常通过(白名单不得误伤)。
    for good_symbol in ["BTCUSDT", "1000SHIBUSDT", "1INCHUSDT", "BRK-B", "XRP_USDT"] {
        let toml = format!(
            "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [{good_symbol:?}]\n\
             timeframe = [\"1d\"]\n[backtest]\ninitial_balance = 10000.0\n\
             [[strategy]]\ntype = \"sma\"\nperiod = 20\n"
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        assert!(
            cfg.validate().is_ok(),
            "真实品种名 {good_symbol} 必须通过白名单"
        );
    }
}

#[test]
fn validate_rejects_empty_strategy() {
    let cfg: Config = toml::from_str(&make_toml(10000.0, true, false)).unwrap();
    assert!(cfg.validate().is_err());
}

fn make_strategy_toml(period_body: &str) -> String {
    format!(
        "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"sma\"\nperiod = {period_body}\n"
    )
}

#[test]
fn validate_rejects_zero_declared_period() {
    let cfg: Config = toml::from_str(&make_strategy_toml("0")).unwrap();
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_rejects_reversed_range() {
    let cfg: Config =
        toml::from_str(&make_strategy_toml("{ start = 50, end = 10, step = 5 }")).unwrap();
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_rejects_zero_step() {
    let cfg: Config =
        toml::from_str(&make_strategy_toml("{ start = 5, end = 20, step = 0 }")).unwrap();
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_accepts_valid_range() {
    let cfg: Config =
        toml::from_str(&make_strategy_toml("{ start = 5, end = 20, step = 5 }")).unwrap();
    assert!(cfg.validate().is_ok());
}

#[test]
fn unknown_field_is_rejected() {
    let toml = format!("{}\nbacktest_typo = 1\n", make_toml(10000.0, true, true));
    assert!(toml::from_str::<Config>(&toml).is_err());
}

#[test]
fn acronym_strategy_names_parse_as_written_in_run_configs() {
    // 回归: ATRBand/BBMR/RSI2 的自然名必须可解析(serde 默认会转成 a_t_r_band/b_b_m_r/r_s_i2)。
    for st in [
        "type = \"atr_band\"\nmult = { start = 1.5, end = 2.5, step = 0.5 }",
        "type = \"bbmr\"\nsigma = { start = 2.0, end = 3.0, step = 0.5 }",
        "type = \"rsi2\"\nbuy = { start = 5.0, end = 10.0, step = 5.0 }\nsell = { start = 90.0, end = 95.0, step = 5.0 }",
        "type = \"bbrk\"\nsigma = { start = 1.5, end = 2.5, step = 0.5 }",
        "type = \"bbrk_adx\"\nsigma = { start = 2.0, end = 3.0, step = 0.5 }\nadx = { start = 20.0, end = 25.0, step = 5.0 }",
        "type = \"bbrk_trail\"\nsigma = { start = 2.0, end = 3.0, step = 0.5 }\ntrailk = { start = 2.0, end = 3.0, step = 1.0 }",
        "type = \"bbrk_don\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nexitn = { start = 2, end = 3, step = 1 }",
        "type = \"bbrk_pctb\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nexitb = { start = 0.2, end = 0.5, step = 0.3 }",
        "type = \"bbrk_atr\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\natrthr = { start = 0.8, end = 1.0, step = 0.2 }",
        "type = \"bbrk_str\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nbody = { start = 0.3, end = 0.5, step = 0.2 }",
        "type = \"bbrk_sma\"\nsigma = 2.5\nsmareg = 100",
        "type = \"bbrk_hma\"\nsigma = 2.5\nhmalen = 100",
        "type = \"bbrk_max\"\nma = [\"sma\", \"hma\"]\nmalen = 100",
        "type = \"bbrk_bb\"\nma = [\"sma\"]\nbblen = 20\nsigma = 2.5",
        "type = \"bbrk_vol_ema\"\nsigma = 2.5\nvoln = 10\nvolmult = 1.5",
        "type = \"bbrk_atr_ema\"\nsigma = 2.5\natrthr = 1.0",
        "type = \"bbrk_sma_ema\"\nsigma = 2.5\nsmareg = 100",
        "type = \"bbrk_hma_ema\"\nsigma = 2.5\nhmalen = 100",
        "type = \"bbrk_hma_v1\"\nsigma = 2.5\nbblen = 20\nhmalen = 450",
        "type = \"bbrk_hma_v2\"\nsigma = 2.5\nbblen = 20\nhmalen = 450",
        "type = \"bbrk_hma_v3\"\nsigma = 2.5\nbblen = 20",
        "type = \"bbrk_hma_v4\"\nsigma = 2.5\nbblen = 20",
        "type = \"bbrk_hma_v5\"\nsigma = 2.5\nbblen = 20\nhmalen = 450",
        "type = \"mabb_long\"\nma = [\"sma\"]\nbblen = 20\nsigma = 2.5\nstopk = 0.0\natrlen = 14",
        "type = \"mabb_short\"\nma = [\"ema\"]\nbblen = 20\nsigma = 2.5\nstopk = 2.0\natrlen = 14",
        "type = \"bbrk_zlema_ema\"\nsigma = 2.5\nzlemareg = 100",
        "type = \"bbrk_rex\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nrexn = { start = 3, end = 5, step = 2 }\nrexmult = 1.5",
        "type = \"bbrk_vol\"\nsigma = 2.5\nvoln = 10\nvolmult = 1.5",
        "type = \"bbrk_ttm\"\nsigma = 2.5\nkc = 1.5\nttmn = 5",
        "type = \"bbrk_ttm_fire\"\nsigma = 2.5\nkc = 1.5",
        "type = \"vwap_trend\"\nreset = { start = 0.0, end = 1.0, step = 1.0 }",
        "type = \"ma_atr\"\nma = [\"sma\"]\nlen = 20\nmult = 2.0\natrlen = 14",
    ] {
        let toml = format!(
            "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\n{st}\n"
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        assert!(cfg.validate().is_ok());
    }
}

fn make_scan_toml(symbol: &str, timeframe: &str) -> String {
    format!(
        "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = {symbol}\ntimeframe = {timeframe}\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"ssf_st\"\nperiod = {{ start = 2, end = 6, step = 1 }}\n"
    )
}

#[test]
fn single_string_symbol_normalizes_to_one_element_list() {
    let cfg: Config = toml::from_str(&make_scan_toml("\"BTCUSDT\"", "\"15m\"")).unwrap();
    assert_eq!(cfg.data.symbols(), ["BTCUSDT".to_string()]);
    assert_eq!(cfg.data.timeframes(), ["15m".to_string()]);
    assert!(cfg.validate().is_ok());
}

#[test]
fn multi_symbol_and_timeframe_lists_are_parsed() {
    let cfg: Config = toml::from_str(&make_scan_toml(
        "[\"BTCUSDT\", \"ETHUSDT\", \"SOLUSDT\"]",
        "[\"15m\", \"1h\", \"4h\", \"1d\"]",
    ))
    .unwrap();
    assert_eq!(cfg.data.symbols().len(), 3);
    assert_eq!(cfg.data.timeframes().len(), 4);
    assert!(cfg.validate().is_ok());
}

#[test]
fn empty_lists_are_rejected_by_validate() {
    let cfg: Config = toml::from_str(&make_scan_toml("[]", "[\"15m\"]")).unwrap();
    assert!(cfg.validate().is_err());

    let cfg2: Config = toml::from_str(&make_scan_toml("[\"BTCUSDT\"]", "[]")).unwrap();
    assert!(cfg2.validate().is_err());
}

#[test]
fn unknown_timeframe_is_rejected_instead_of_silently_annualized_as_1d() {
    // 3m 曾不在 bars_per_year 表里, 会静默按 365 年化(差 480 倍)
    for tf in ["3m", "8h", "4h", "1m", "1w"] {
        let cfg: Config =
            toml::from_str(&make_scan_toml("\"BTCUSDT\"", &format!("\"{tf}\""))).unwrap();
        assert!(cfg.validate().is_ok(), "{tf} 应被接受");
    }
    let cfg: Config = toml::from_str(&make_scan_toml("\"BTCUSDT\"", "\"3s\"")).unwrap();
    assert!(cfg.validate().is_err());
}

#[test]
fn for_pair_produces_single_value_config() {
    let cfg: Config = toml::from_str(&make_scan_toml(
        "[\"BTCUSDT\", \"ETHUSDT\"]",
        "[\"15m\", \"1h\"]",
    ))
    .unwrap();
    let pair = cfg.data.for_pair("ETHUSDT", "1h").unwrap();
    assert_eq!(pair.symbol(), "ETHUSDT");
    assert_eq!(pair.timeframe(), "1h");
    assert_eq!(pair.dir(), cfg.data.dir);
    assert_eq!(pair.source_type(), cfg.data.source_type);
    assert_eq!(pair.market(), crate::data::Market::Um);
}

#[test]
fn for_pair_honours_market_override() {
    let toml = "[data]\ntype = \"binance\"\nmarket = \"spot\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"sma\"\nperiod = 20\n";
    let cfg: Config = toml::from_str(toml).unwrap();
    let pair = cfg.data.for_pair("BTCUSDT", "1d").unwrap();
    assert_eq!(pair.market(), crate::data::Market::Spot);
}

#[test]
fn for_pair_preserves_date_window() {
    let toml = "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\ntimeframe = [\"15m\"]\nstart_date = \"2024-01-01\"\nend_date = \"2024-06-30\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"ssf_st\"\nperiod = 3\n";
    let cfg: Config = toml::from_str(toml).unwrap();
    let pair = cfg.data.for_pair("BTCUSDT", "15m").unwrap();
    assert_eq!(
        pair.start_date().map(|d| d.to_string()).as_deref(),
        Some("2024-01-01")
    );
    assert_eq!(
        pair.end_date().map(|d| d.to_string()).as_deref(),
        Some("2024-06-30")
    );
}

#[test]
fn fill_bar_price_defaults_to_open_when_omitted() {
    let toml = "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"sma\"\nperiod = 20\n";
    let cfg: Config = toml::from_str(toml).unwrap();
    assert_eq!(cfg.backtest.fill_bar_price, FillBarPrice::Open);
}

#[test]
fn fill_bar_price_accepts_close_and_rejects_unknown() {
    let toml = "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\nfill_bar_price = \"close\"\n\n[[strategy]]\ntype = \"sma\"\nperiod = 20\n";
    let cfg: Config = toml::from_str(toml).unwrap();
    assert_eq!(cfg.backtest.fill_bar_price, FillBarPrice::Close);
    let bad = toml.replace("\"close\"", "\"mid\"");
    assert!(toml::from_str::<Config>(&bad).is_err());
}

fn make_stop_toml(stop_body: &str) -> String {
    format!(
        "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n\
         [backtest]\ninitial_balance = 10000.0\n\n\
         [[strategy]]\ntype = \"sma\"\nperiod = 20\n{stop_body}"
    )
}

#[test]
fn strategy_stop_parses_inline_and_subtable_forms() {
    let inline = make_stop_toml("stop = { stop_loss_pct = 2.0, trailing_stop_pct = 5.0 }\n");
    let cfg: Config = toml::from_str(&inline).unwrap();
    assert_eq!(cfg.strategy[0].stop.stop_loss_pct, Some(2.0));
    assert_eq!(cfg.strategy[0].stop.trailing_stop_pct, Some(5.0));
    assert!(cfg.validate().is_ok());

    let subtable = make_stop_toml("[strategy.stop]\nstop_loss_pct = 2.0\n");
    let cfg: Config = toml::from_str(&subtable).unwrap();
    assert_eq!(cfg.strategy[0].stop.stop_loss_pct, Some(2.0));
    assert_eq!(cfg.strategy[0].stop.trailing_stop_pct, None);
    assert!(cfg.validate().is_ok());
}

#[test]
fn strategy_stop_is_per_entry_and_defaults_to_none() {
    let toml = format!(
        "{}\n[[strategy]]\ntype = \"sma\"\nperiod = 20\nstop = {{ stop_loss_pct = 3.0 }}\n",
        make_stop_toml("")
    );
    let cfg: Config = toml::from_str(&toml).unwrap();
    assert_eq!(cfg.strategy.len(), 2);
    assert!(cfg.strategy[0].stop.is_none());
    assert_eq!(cfg.strategy[1].stop.stop_loss_pct, Some(3.0));
    assert!(cfg.validate().is_ok());
}

#[test]
fn strategy_stop_parses_atr_and_breakeven_rules() {
    let toml = make_stop_toml(
        "stop = { atr = { period = 14, mult = 2.0, trailing = true }, \
         breakeven = { trigger_pct = 3.0, offset_pct = 0.5 } }\n",
    );
    let cfg: Config = toml::from_str(&toml).unwrap();
    let stop = &cfg.strategy[0].stop;
    assert_eq!(
        stop.atr,
        Some(crate::engine::AtrStopConfig {
            period: 14,
            mult: 2.0,
            trailing: true,
        })
    );
    assert_eq!(
        stop.breakeven,
        Some(crate::engine::BreakevenConfig {
            trigger_pct: 3.0,
            offset_pct: 0.5,
        })
    );
    assert!(!stop.is_none());
    assert!(cfg.validate().is_ok());
    let params = stop.param_str();
    assert!(params.contains("atr_period=14"));
    assert!(params.contains("atr_mult=2"));
    assert!(params.contains("atr_trailing=true"));
    assert!(params.contains("breakeven_trigger_pct=3"));
    assert!(params.contains("breakeven_offset_pct=0.5"));

    // 省略 `trailing` 默认为固定 ATR 止损; 省略 `offset_pct` 默认为 0。
    let toml = make_stop_toml(
        "stop = { atr = { period = 14, mult = 2.0 }, breakeven = { trigger_pct = 3.0 } }\n",
    );
    let cfg: Config = toml::from_str(&toml).unwrap();
    assert!(!cfg.strategy[0].stop.atr.unwrap().trailing);
    assert_eq!(cfg.strategy[0].stop.breakeven.unwrap().offset_pct, 0.0);
}

#[test]
fn strategy_stop_rejects_bad_atr_and_breakeven_values() {
    for bad in [
        "stop = { atr = { period = 0, mult = 2.0 } }\n",
        "stop = { atr = { period = 14, mult = 0.0 } }\n",
        "stop = { atr = { period = 14, mult = nan } }\n",
        "stop = { breakeven = { trigger_pct = 0.0 } }\n",
        "stop = { breakeven = { trigger_pct = 100.0 } }\n",
        "stop = { breakeven = { trigger_pct = 3.0, offset_pct = -1.0 } }\n",
        "stop = { breakeven = { trigger_pct = 3.0, offset_pct = 100.0 } }\n",
    ] {
        let cfg: Config = toml::from_str(&make_stop_toml(bad)).unwrap();
        assert!(cfg.validate().is_err(), "非法止损必须被拒绝: {bad}");
    }

    for missing in [
        "stop = { atr = { period = 14 } }\n",
        "stop = { atr = { mult = 2.0 } }\n",
        "stop = { breakeven = { offset_pct = 0.0 } }\n",
        "stop = { atr = { period = 14, mult = 2.0, bogus = 1 } }\n",
        "stop = { breakeven = { trigger_pct = 3.0, bogus = 1 } }\n",
    ] {
        assert!(
            toml::from_str::<Config>(&make_stop_toml(missing)).is_err(),
            "缺失/未知嵌套键必须被拒绝: {missing}"
        );
    }
}

#[test]
fn strategy_stop_rejects_unknown_keys_and_out_of_range_values() {
    let typo = make_stop_toml("stop = { stop_los_pct = 2.0 }\n");
    assert!(
        toml::from_str::<Config>(&typo).is_err(),
        "stop 键名拼错必须报错"
    );

    for bad in [
        "stop = { stop_loss_pct = 0.0 }\n",
        "stop = { stop_loss_pct = 100.0 }\n",
        "stop = { trailing_stop_pct = -1.0 }\n",
        "stop = { trailing_stop_pct = nan }\n",
    ] {
        let cfg: Config = toml::from_str(&make_stop_toml(bad)).unwrap();
        assert!(cfg.validate().is_err(), "非法止损必须被拒绝: {bad}");
    }
}

#[test]
fn strategy_stop_does_not_weaken_unknown_strategy_field_check() {
    let bad = make_stop_toml("bogus = 1\nstop = { stop_loss_pct = 2.0 }\n");
    assert!(
        toml::from_str::<Config>(&bad).is_err(),
        "flatten 不得让策略拼错字段静默通过"
    );
}

#[test]
fn strategy_sizing_parses_all_modes_and_defaults_to_all_in() {
    use crate::engine::SizingConfig;

    let cfg: Config = toml::from_str(&make_stop_toml("")).unwrap();
    assert_eq!(cfg.strategy[0].sizing, SizingConfig::AllIn);

    let cfg: Config = toml::from_str(&make_stop_toml(
        "sizing = { type = \"fixed\", fraction_pct = 25.0 }\n",
    ))
    .unwrap();
    assert_eq!(
        cfg.strategy[0].sizing,
        SizingConfig::Fixed { fraction_pct: 25.0 }
    );
    assert!(cfg.validate().is_ok());

    let cfg: Config = toml::from_str(&make_stop_toml(
        "sizing = { type = \"kelly\", lookback = 50, min_trades = 20, kelly_fraction = 0.5 }\n",
    ))
    .unwrap();
    assert_eq!(
        cfg.strategy[0].sizing,
        SizingConfig::Kelly {
            lookback: 50,
            min_trades: 20,
            kelly_fraction: 0.5,
            max_position_pct: 100.0,
            fallback_fraction_pct: 10.0,
        }
    );

    let cfg: Config = toml::from_str(&make_stop_toml(
        "[strategy.sizing]\ntype = \"atr\"\nrisk_pct = 1.0\nperiod = 14\nmult = 2.0\n",
    ))
    .unwrap();
    assert_eq!(
        cfg.strategy[0].sizing,
        SizingConfig::Atr {
            risk_pct: 1.0,
            period: 14,
            mult: 2.0,
            max_position_pct: 100.0,
            fallback_fraction_pct: 0.0,
        }
    );
    assert!(cfg.validate().is_ok());
}

#[test]
fn strategy_sizing_rejects_bad_values() {
    for bad in [
        "sizing = { type = \"fixed\", fraction_pct = 0.0 }\n",
        "sizing = { type = \"fixed\", fraction_pct = 100.5 }\n",
        "sizing = { type = \"fixed\", fraction_pct = nan }\n",
        "sizing = { type = \"kelly\", lookback = 0, min_trades = 2, kelly_fraction = 0.5 }\n",
        "sizing = { type = \"kelly\", lookback = 10, min_trades = 1, kelly_fraction = 0.5 }\n",
        "sizing = { type = \"kelly\", lookback = 10, min_trades = 11, kelly_fraction = 0.5 }\n",
        "sizing = { type = \"kelly\", lookback = 10, min_trades = 2, kelly_fraction = 0.0 }\n",
        "sizing = { type = \"kelly\", lookback = 10, min_trades = 2, kelly_fraction = 1.5 }\n",
        "sizing = { type = \"kelly\", lookback = 10, min_trades = 2, kelly_fraction = 0.5, fallback_fraction_pct = 0.0 }\n",
        "sizing = { type = \"atr\", risk_pct = 0.0, period = 14, mult = 2.0 }\n",
        "sizing = { type = \"atr\", risk_pct = 1.0, period = 0, mult = 2.0 }\n",
        "sizing = { type = \"atr\", risk_pct = 1.0, period = 14, mult = 0.0 }\n",
        "sizing = { type = \"atr\", risk_pct = 1.0, period = 14, mult = 2.0, fallback_fraction_pct = 101.0 }\n",
    ] {
        let cfg: Config = toml::from_str(&make_stop_toml(bad)).unwrap();
        assert!(cfg.validate().is_err(), "非法仓位必须被拒绝: {bad}");
    }

    for bad in [
        "sizing = { type = \"fixed\" }\n",
        "sizing = { type = \"fixed\", fraction_pct = 20.0, bogus = 1 }\n",
        "sizing = { type = \"bogus\" }\n",
    ] {
        assert!(
            toml::from_str::<Config>(&make_stop_toml(bad)).is_err(),
            "缺失/未知仓位键必须被拒绝: {bad}"
        );
    }
}
