use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;
use crate::strategies::ssf::SsfConfig;
use crate::strategies::ssf_cd::SsfCdConfig;
use crate::strategies::ssf_st::SsfStConfig;
use crate::strategies::ssf_st_cd::SsfStCdConfig;

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
            sc.period_param()
                .ok_or_else(|| AppError::Msg("strategy 缺少 period 参数(无法解析)".into()))?
                .validate()?;
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
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacktestConfig {
    pub initial_balance: f64,
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
}
