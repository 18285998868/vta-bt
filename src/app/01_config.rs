use serde::Deserialize;

use crate::strategies::ssf::SsfConfig;
use crate::strategies::ssf_st::SsfStConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub data: lib_reader::DataConfig,
    pub backtest: BacktestConfig,
    pub output: Option<OutputConfig>,
    pub strategy: Vec<StrategyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BacktestConfig {
    pub initial_balance: f64,
}

/// 输出配置。`dir` 为输出根目录，合并结果写入 `{dir}/result.parquet`，逐品种写入 `{dir}/temps/`。
/// 若未设置，默认使用当前目录 `./reports/`。
#[derive(Debug, Clone, Deserialize)]
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
pub struct MaGridCfg {
    pub period: PeriodParam,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    Sma(MaGridCfg),
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
    SsfSt(SsfStConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PeriodParam {
    Single(usize),
    Range(RangeParam<usize>),
}

impl PeriodParam {
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
