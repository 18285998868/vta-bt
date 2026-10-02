use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;
use crate::strategies::aberration::AberrationConfig;
use crate::strategies::atrband::ATRBandConfig;
use crate::strategies::bbmr::BBMRConfig;
use crate::strategies::bbrk::BBRKConfig;
use crate::strategies::dualthrust::DualThrustConfig;
use crate::strategies::ema_cross::EmaCrossConfig;
use crate::strategies::fiali::FialiConfig;
use crate::strategies::keltner::KeltnerConfig;
use crate::strategies::rbreaker::RBreakerConfig;
use crate::strategies::rsi2::RSI2Config;
use crate::strategies::ssf::SsfConfig;
use crate::strategies::ssf_cd::SsfCdConfig;
use crate::strategies::ssf_st::SsfStConfig;
use crate::strategies::ssf_st_cd::SsfStCdConfig;
use crate::strategies::turtle::TurtleConfig;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data: DataSection,
    pub backtest: BacktestConfig,
    pub output: Option<OutputConfig>,
    pub strategy: Vec<StrategyConfig>,
}

/// 单值或列表, 统一归一化为 `Vec<String>`。
/// TOML 里 `symbol = "BTCUSDT"` 与 `symbol = ["BTCUSDT", "ETHUSDT"]` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn items(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

/// vta 自己的数据配置: 与 `lib_reader::DataConfig` 字段一致, 但 symbol/timeframe 支持列表。
/// 加载时通过 [`DataSection::for_pair`] 降为 lib-reader 需要的单值形态。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSection {
    #[serde(rename = "type")]
    pub source_type: lib_reader::DataSource,
    pub dir: PathBuf,
    pub symbol: OneOrMany,
    pub timeframe: OneOrMany,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

impl DataSection {
    pub fn symbols(&self) -> Vec<String> {
        self.symbol.items()
    }

    pub fn timeframes(&self) -> Vec<String> {
        self.timeframe.items()
    }

    /// 取出一组 (symbol, timeframe) 对应的单值配置, 供 `lib_reader::load_columns` 使用。
    pub fn for_pair(&self, symbol: &str, timeframe: &str) -> lib_reader::DataConfig {
        lib_reader::DataConfig {
            source_type: self.source_type,
            dir: self.dir.clone(),
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.backtest.initial_balance <= 0.0 {
            return Err(AppError::Msg(
                "初始资金 backtest.initial_balance 必须大于 0".into(),
            ));
        }
        if self.backtest.fee_bps < 0.0 || self.backtest.slippage_bps < 0.0 {
            return Err(AppError::Msg(
                "backtest.fee_bps / slippage_bps 不能为负".into(),
            ));
        }
        if self.data.symbols().iter().any(|s| s.is_empty()) || self.data.symbols().is_empty() {
            return Err(AppError::Msg("data.symbol 不能为空".into()));
        }
        let tfs = self.data.timeframes();
        if tfs.is_empty() || tfs.iter().any(|s| s.is_empty()) {
            return Err(AppError::Msg("data.timeframe 不能为空".into()));
        }
        for tf in &tfs {
            if !crate::strategies::is_known_timeframe(tf) {
                return Err(AppError::Msg(format!(
                    "未知周期 data.timeframe = {tf:?}; 已知: 1m/3m/5m/15m/30m/1h/2h/4h/6h/8h/12h/1d/1w"
                )));
            }
        }
        if self.strategy.is_empty() {
            return Err(AppError::Msg("strategy 不能为空".into()));
        }
        for sc in &self.strategy {
            match sc {
                // 双均线用 fast/slow 双参数, 无单一 period。
                StrategyConfig::EmaCross(c) => {
                    c.fast.validate()?;
                    c.slow.validate()?;
                    if c.pairs().is_empty() {
                        return Err(AppError::Msg("ema_cross 无 fast<slow 有效组合".into()));
                    }
                }
                StrategyConfig::Turtle(c) => {
                    for pp in [&c.entry, &c.exit] {
                        pp.validate()?;
                    }
                    if c.pairs().is_empty() {
                        return Err(AppError::Msg("turtle 参数组合为空".into()));
                    }
                }
                StrategyConfig::DualThrust(c) => {
                    c.n.validate()?;
                    c.k.validate_range("dualthrust.k", 0.01, 5.0)?;
                    if c.pairs().is_empty() {
                        return Err(AppError::Msg("dualthrust 参数组合为空".into()));
                    }
                }
                StrategyConfig::RBreaker(c) => {
                    c.stop_pct.validate_range("rbreaker.stop_pct", 0.01, 50.0)?;
                    if c.values().is_empty() {
                        return Err(AppError::Msg("rbreaker 参数组合为空".into()));
                    }
                }
                StrategyConfig::Fiali(_) => {}
                StrategyConfig::Aberration(c) => {
                    c.n.validate()?;
                    c.mult.validate_range("aberration.mult", 0.1, 20.0)?;
                    if c.pairs().is_empty() {
                        return Err(AppError::Msg("aberration 参数组合为空".into()));
                    }
                }
                StrategyConfig::ATRBand(c) => {
                    c.mult.validate_range("atrband.mult", 0.1, 20.0)?;
                    if c.values().is_empty() {
                        return Err(AppError::Msg("atrband 参数组合为空".into()));
                    }
                }
                StrategyConfig::Keltner(c) => {
                    c.mult.validate_range("keltner.mult", 0.1, 20.0)?;
                    if c.values().is_empty() {
                        return Err(AppError::Msg("keltner 参数组合为空".into()));
                    }
                }
                StrategyConfig::BBMR(c) => {
                    c.sigma.validate_range("bbmr.sigma", 0.1, 10.0)?;
                    if c.values().is_empty() {
                        return Err(AppError::Msg("bbmr 参数组合为空".into()));
                    }
                }
                StrategyConfig::Bbrk(c) => {
                    c.sigma.validate_range("bbrk.sigma", 0.1, 10.0)?;
                    if c.values().is_empty() {
                        return Err(AppError::Msg("bbrk 参数组合为空".into()));
                    }
                }
                StrategyConfig::RSI2(c) => {
                    c.buy.validate_range("rsi2.buy", 0.0, 100.0)?;
                    c.sell.validate_range("rsi2.sell", 0.0, 100.0)?;
                    if c.pairs().is_empty() {
                        return Err(AppError::Msg("rsi2 参数组合为空".into()));
                    }
                }
                _ => {
                    sc.period_param()
                        .ok_or_else(|| AppError::Msg("strategy 缺少 period 参数(无法解析)".into()))?
                        .validate()?;
                }
            }
        }
        Ok(())
    }
}

impl StrategyConfig {
    fn period_param(&self) -> Option<&PeriodParam> {
        match self {
            StrategyConfig::Sma(c)
            | StrategyConfig::SmaCd(c)
            | StrategyConfig::Ema(c)
            | StrategyConfig::Hma(c)
            | StrategyConfig::Zlma(c)
            | StrategyConfig::Alma(c)
            | StrategyConfig::Dema(c)
            | StrategyConfig::Fwma(c)
            | StrategyConfig::Jma(c)
            | StrategyConfig::Kama(c)
            | StrategyConfig::LinReg(c)
            | StrategyConfig::Pwma(c)
            | StrategyConfig::SinWma(c)
            | StrategyConfig::Swma(c)
            | StrategyConfig::Tema(c)
            | StrategyConfig::Tilson(c)
            | StrategyConfig::Trima(c)
            | StrategyConfig::Wilders(c)
            | StrategyConfig::Wma(c)
            | StrategyConfig::Vwma(c)
            | StrategyConfig::Vidya(c)
            | StrategyConfig::McGinley(c) => Some(&c.period),
            StrategyConfig::Ssf(c) => Some(&c.period),
            StrategyConfig::SsfCd(c) => Some(&c.period),
            StrategyConfig::SsfSt(c) => Some(&c.period),
            StrategyConfig::SsfStCd(c) => Some(&c.period),
            StrategyConfig::Turtle(_)
            | StrategyConfig::DualThrust(_)
            | StrategyConfig::RBreaker(_)
            | StrategyConfig::Fiali(_)
            | StrategyConfig::Aberration(_)
            | StrategyConfig::ATRBand(_)
            | StrategyConfig::Keltner(_)
            | StrategyConfig::BBMR(_)
            | StrategyConfig::Bbrk(_)
            | StrategyConfig::RSI2(_) => None,
            // 双均线无单一 period, 调用方走 validate() 的 EmaCross 分支。
            StrategyConfig::EmaCross(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacktestConfig {
    pub initial_balance: f64,
    /// 信号到成交的延迟(bar 数)。0 = 现状: 信号当根收盘成交(前视乐观);
    /// 1 = 信号次根 fill bar 成交(推荐, 消除同 bar 前视)。
    #[serde(default)]
    pub delay_bars: usize,
    /// 单边佣金(bp)。如 taker 2bp 填 2.0, maker 零费填 0。
    #[serde(default)]
    pub fee_bps: f64,
    /// 单边逆向滑点(bp)。买入抬价、卖出压价; 与 fee 叠加扣除。
    #[serde(default)]
    pub slippage_bps: f64,
    /// delay>0 时 fill bar 的成交价: "open"(默认, 次根开盘)或 "close"(次根收盘)。
    /// 旧配置文件无此键时回退为 open, 历史结果不受影响。
    #[serde(default)]
    pub fill_bar_price: FillBarPrice,
}

/// delay>0 时 fill bar 的成交价位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FillBarPrice {
    /// fill bar 开盘(默认, 现状口径)。
    #[default]
    Open,
    /// fill bar 收盘(多等一根 bar 内漂移, 更贴近追单成交)。
    Close,
}

/// 输出配置。`dir` 为输出根目录，合并结果写入 `{dir}/result.parquet`，逐组合写入 `{dir}/temps/`。
/// 若未设置，默认使用当前目录 `./reports/`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    pub dir: Option<String>,
    /// 是否导出逐笔交易和权益曲线详细数据
    #[serde(default)]
    pub detail: bool,
    /// 是否覆盖已存在的回测结果文件，默认 false（跳过已完成的组合）
    #[serde(default)]
    pub overwrite: bool,
}

/// Config for single-MA price-cross strategies.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaGridCfg {
    pub period: PeriodParam,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StrategyConfig {
    Sma(MaGridCfg),
    SmaCd(MaGridCfg),
    Ema(MaGridCfg),
    Hma(MaGridCfg),
    Zlma(MaGridCfg),
    Alma(MaGridCfg),
    Dema(MaGridCfg),
    Fwma(MaGridCfg),
    Jma(MaGridCfg),
    Kama(MaGridCfg),
    LinReg(MaGridCfg),
    Pwma(MaGridCfg),
    SinWma(MaGridCfg),
    Swma(MaGridCfg),
    Tema(MaGridCfg),
    Tilson(MaGridCfg),
    Trima(MaGridCfg),
    Wilders(MaGridCfg),
    Wma(MaGridCfg),
    Vwma(MaGridCfg),
    Vidya(MaGridCfg),
    McGinley(MaGridCfg),
    Ssf(SsfConfig),
    SsfCd(SsfCdConfig),
    SsfSt(SsfStConfig),
    SsfStCd(SsfStCdConfig),
    Turtle(TurtleConfig),
    DualThrust(DualThrustConfig),
    RBreaker(RBreakerConfig),
    Fiali(FialiConfig),
    Aberration(AberrationConfig),
    #[serde(rename = "atr_band")]
    ATRBand(ATRBandConfig),
    Keltner(KeltnerConfig),
    #[serde(rename = "bbmr")]
    BBMR(BBMRConfig),
    Bbrk(BBRKConfig),
    #[serde(rename = "rsi2")]
    RSI2(RSI2Config),
    EmaCross(EmaCrossConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PeriodParam {
    Single(usize),
    Range(RangeParam<usize>),
}

impl PeriodParam {
    pub fn validate(&self) -> crate::error::Result<()> {
        match self {
            PeriodParam::Single(v) => {
                if *v < 1 {
                    return Err(AppError::Msg(format!("period 必须 >= 1, 当前为 {v}")));
                }
            }
            PeriodParam::Range(r) => {
                if r.start < 1 {
                    return Err(AppError::Msg(format!(
                        "period 必须 >= 1, 当前 start = {}",
                        r.start
                    )));
                }
                if r.start > r.end {
                    return Err(AppError::Msg(format!(
                        "period 起点 {} 不能大于终点 {}",
                        r.start, r.end
                    )));
                }
                if r.step == 0 {
                    return Err(AppError::Msg(format!(
                        "period.step 必须 > 0, 当前为 {}",
                        r.step
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn into_range(self) -> RangeParam<usize> {
        match self {
            PeriodParam::Single(v) => RangeParam {
                start: v,
                end: v,
                step: 1,
            },
            PeriodParam::Range(r) => r,
        }
    }

    pub fn to_range(&self) -> RangeParam<usize> {
        match self {
            PeriodParam::Single(v) => RangeParam {
                start: *v,
                end: *v,
                step: 1,
            },
            PeriodParam::Range(r) => r.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangeParam<T: PartialOrd + Copy> {
    pub start: T,
    pub end: T,
    pub step: T,
}

/// f64 参数: 单值或范围(与 PeriodParam 对应, 后者只支持 usize)。
/// TOML 里 `k = 0.2` 与 `k = { start = 0.1, end = 0.6, step = 0.1 }` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum F64Param {
    Single(f64),
    Range(RangeParam<f64>),
}

impl F64Param {
    pub fn values(&self) -> Vec<f64> {
        match self {
            F64Param::Single(v) => vec![*v],
            F64Param::Range(r) => crate::strategies::expand_f64_range(r.start, r.end, r.step),
        }
    }

    pub fn validate_range(&self, name: &str, lo: f64, hi: f64) -> crate::error::Result<()> {
        let vs = self.values();
        if vs.is_empty() {
            return Err(AppError::Msg(format!("{name} 展开为空")));
        }
        if vs.iter().any(|v| !v.is_finite() || *v < lo || *v > hi) {
            return Err(AppError::Msg(format!("{name} 越界(允许 [{lo}, {hi}])")));
        }
        Ok(())
    }
}

pub fn load_config(path: &str) -> crate::error::Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn validate_rejects_zero_balance() {
        let cfg: Config = toml::from_str(&make_toml(0.0, true, true)).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_symbols() {
        let cfg: Config = toml::from_str(&make_toml(10000.0, false, true)).unwrap();
        assert!(cfg.validate().is_err());
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
        assert_eq!(cfg.data.symbols(), vec!["BTCUSDT".to_string()]);
        assert_eq!(cfg.data.timeframes(), vec!["15m".to_string()]);
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
    fn for_pair_produces_single_value_config_for_lib_reader() {
        let cfg: Config = toml::from_str(&make_scan_toml(
            "[\"BTCUSDT\", \"ETHUSDT\"]",
            "[\"15m\", \"1h\"]",
        ))
        .unwrap();
        let pair = cfg.data.for_pair("ETHUSDT", "1h");
        assert_eq!(pair.symbol, "ETHUSDT");
        assert_eq!(pair.timeframe, "1h");
        assert_eq!(pair.dir, cfg.data.dir);
        assert_eq!(pair.source_type, cfg.data.source_type);
    }

    #[test]
    fn for_pair_preserves_date_window() {
        let toml = "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\ntimeframe = [\"15m\"]\nstart_date = \"2024-01-01\"\nend_date = \"2024-06-30\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"ssf_st\"\nperiod = 3\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let pair = cfg.data.for_pair("BTCUSDT", "15m");
        assert_eq!(pair.start_date.as_deref(), Some("2024-01-01"));
        assert_eq!(pair.end_date.as_deref(), Some("2024-06-30"));
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
}
