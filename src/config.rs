use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;
use crate::strategies::aberration::AberrationConfig;
use crate::strategies::atma::AtmaConfig;
use crate::strategies::atrband::ATRBandConfig;
use crate::strategies::bbmr::BBMRConfig;
use crate::strategies::bbrk::BBRKConfig;
use crate::strategies::bbrk_adx::BbrkAdxConfig;
use crate::strategies::bbrk_atr::BbrkAtrConfig;
use crate::strategies::bbrk_atr_ema::BbrkAtrEmaConfig;
use crate::strategies::bbrk_bb::BbrkBbConfig;
use crate::strategies::bbrk_cross::BbrkCrossConfig;
use crate::strategies::bbrk_don::BbrkDonConfig;
use crate::strategies::bbrk_ema::BbrkEmaConfig;
use crate::strategies::bbrk_hma::BbrkHmaConfig;
use crate::strategies::bbrk_hma_ema::BbrkHmaEmaConfig;
use crate::strategies::bbrk_hma_v1::BbrkHmaV1Config;
use crate::strategies::bbrk_hma_v1_sl::BbrkHmaV1SlConfig;
use crate::strategies::bbrk_hma_v2::BbrkHmaV2Config;
use crate::strategies::bbrk_hma_v2_sl::BbrkHmaV2SlConfig;
use crate::strategies::bbrk_hma_v3::BbrkHmaV3Config;
use crate::strategies::bbrk_hma_v3_sl::BbrkHmaV3SlConfig;
use crate::strategies::bbrk_hma_v4::BbrkHmaV4Config;
use crate::strategies::bbrk_hma_v4_sl::BbrkHmaV4SlConfig;
use crate::strategies::bbrk_hma_v5::BbrkHmaV5Config;
use crate::strategies::bbrk_hma_v5_sl::BbrkHmaV5SlConfig;
use crate::strategies::bbrk_kdj::BbrkKdjConfig;
use crate::strategies::bbrk_m2::BbrkM2Config;
use crate::strategies::bbrk_m3::BbrkM3Config;
use crate::strategies::bbrk_max::BbrkMaxConfig;
use crate::strategies::bbrk_orth::BbrkOrthConfig;
use crate::strategies::bbrk_pctb::BbrkPctbConfig;
use crate::strategies::bbrk_rex::BbrkRexConfig;
use crate::strategies::bbrk_sma::BbrkSmaConfig;
use crate::strategies::bbrk_sma_ema::BbrkSmaEmaConfig;
use crate::strategies::bbrk_str::BbrkStrConfig;
use crate::strategies::bbrk_trail::BbrkTrailConfig;
use crate::strategies::bbrk_ttm::BbrkTtmConfig;
use crate::strategies::bbrk_ttm_fire::BbrkTtmFireConfig;
use crate::strategies::bbrk_vol::BbrkVolConfig;
use crate::strategies::bbrk_vol_ema::BbrkVolEmaConfig;
use crate::strategies::bbrk_zlema_ema::BbrkZlemaEmaConfig;
use crate::strategies::dualthrust::DualThrustConfig;
use crate::strategies::edma::EdmaConfig;
use crate::strategies::ema_cross::EmaCrossConfig;
use crate::strategies::fiali::FialiConfig;
use crate::strategies::keltner::KeltnerConfig;
use crate::strategies::mabb_long::MabbLongConfig;
use crate::strategies::mabb_short::MabbShortConfig;
use crate::strategies::macd::MacdConfig;
use crate::strategies::rbreaker::RBreakerConfig;
use crate::strategies::rsi2::RSI2Config;
use crate::strategies::sgma::SgmaConfig;
use crate::strategies::ssf::SsfConfig;
use crate::strategies::ssf_cd::SsfCdConfig;
use crate::strategies::ssf_st::SsfStConfig;
use crate::strategies::ssf_st_cd::SsfStCdConfig;
use crate::strategies::turtle::TurtleConfig;
use crate::strategies::vwap::VwapConfig;
use crate::strategies::vwap_trend::VwapTrendConfig;

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

/// vta 自己的数据配置: 与 `kline_reader::DataConfig` 字段一致, 但 symbol/timeframe 支持列表。
/// 加载时通过 [`DataSection::for_pair`] 降为 kline-reader 需要的单值形态。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSection {
    #[serde(rename = "type")]
    pub source_type: kline_reader::DataSource,
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

    /// 取出一组 (symbol, timeframe) 对应的单值配置, 供 `kline_reader::load_columns` 使用。
    /// 已验证构造：非法 symbol/timeframe 即 `Err`，调用方按跳过处理（不 `unwrap`）。
    pub fn for_pair(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> Result<kline_reader::DataConfig, kline_reader::ReaderError> {
        kline_reader::DataConfig::new(
            self.source_type,
            &self.dir,
            symbol,
            timeframe,
            self.start_date.as_deref(),
            self.end_date.as_deref(),
        )
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
        if self.backtest.delay_bars < 1 {
            return Err(AppError::Msg(
                "backtest.delay_bars = 0 已删除(信号当根成交, 前视乐观); 只允许≥1(次根 fill bar 起)".into(),
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
        // 策略专属校验: **下沉到各 config 的 `validate()`**, 此处只做分派。
        //
        // 这个 match **刻意不写 `_` 兜底**: 枚举穷尽性由编译器强制, 新增变体而忘记
        // 接入 validate 会在**编译期**失败。旧实现用
        // `_ => period_param().ok_or_else(...)?.validate()` 兜底, 漏改只会**静默**
        // 少校验(只校验 period), 是隐蔽的正确性陷阱。
        for sc in &self.strategy {
            match sc {
                StrategyConfig::ATRBand(c) => c.validate()?,
                StrategyConfig::Aberration(c) => c.validate()?,
                StrategyConfig::Atma(c) => c.validate()?,
                StrategyConfig::BBMR(c) => c.validate()?,
                StrategyConfig::Bbrk(c) => c.validate()?,
                StrategyConfig::BbrkAdx(c) => c.validate()?,
                StrategyConfig::BbrkAtr(c) => c.validate()?,
                StrategyConfig::BbrkAtrEma(c) => c.validate()?,
                StrategyConfig::BbrkBb(c) => c.validate()?,
                StrategyConfig::BbrkCross(c) => c.validate()?,
                StrategyConfig::BbrkDon(c) => c.validate()?,
                StrategyConfig::BbrkEma(c) => c.validate()?,
                StrategyConfig::BbrkHma(c) => c.validate()?,
                StrategyConfig::BbrkHmaEma(c) => c.validate()?,
                StrategyConfig::BbrkHmaV1(c) => c.validate()?,
                StrategyConfig::BbrkHmaV1Sl(c) => c.validate()?,
                StrategyConfig::BbrkHmaV2(c) => c.validate()?,
                StrategyConfig::BbrkHmaV2Sl(c) => c.validate()?,
                StrategyConfig::BbrkHmaV3(c) => c.validate()?,
                StrategyConfig::BbrkHmaV3Sl(c) => c.validate()?,
                StrategyConfig::BbrkHmaV4(c) => c.validate()?,
                StrategyConfig::BbrkHmaV4Sl(c) => c.validate()?,
                StrategyConfig::BbrkHmaV5(c) => c.validate()?,
                StrategyConfig::BbrkHmaV5Sl(c) => c.validate()?,
                StrategyConfig::BbrkKdj(c) => c.validate()?,
                StrategyConfig::BbrkM2(c) => c.validate()?,
                StrategyConfig::BbrkM3(c) => c.validate()?,
                StrategyConfig::BbrkMax(c) => c.validate()?,
                StrategyConfig::BbrkOrth(c) => c.validate()?,
                StrategyConfig::BbrkPctb(c) => c.validate()?,
                StrategyConfig::BbrkRex(c) => c.validate()?,
                StrategyConfig::BbrkSma(c) => c.validate()?,
                StrategyConfig::BbrkSmaEma(c) => c.validate()?,
                StrategyConfig::BbrkStr(c) => c.validate()?,
                StrategyConfig::BbrkTrail(c) => c.validate()?,
                StrategyConfig::BbrkTtm(c) => c.validate()?,
                StrategyConfig::BbrkTtmFire(c) => c.validate()?,
                StrategyConfig::BbrkVol(c) => c.validate()?,
                StrategyConfig::BbrkVolEma(c) => c.validate()?,
                StrategyConfig::BbrkZlemaEma(c) => c.validate()?,
                StrategyConfig::DualThrust(c) => c.validate()?,
                StrategyConfig::Edma(c) => c.validate()?,
                StrategyConfig::EmaCross(c) => c.validate()?,
                StrategyConfig::Fiali(c) => c.validate()?,
                StrategyConfig::Keltner(c) => c.validate()?,
                StrategyConfig::Alma(c)
                | StrategyConfig::Dema(c)
                | StrategyConfig::Dma(c)
                | StrategyConfig::Ehma(c)
                | StrategyConfig::Ema(c)
                | StrategyConfig::Frama(c)
                | StrategyConfig::Fwma(c)
                | StrategyConfig::Hma(c)
                | StrategyConfig::Jma(c)
                | StrategyConfig::Kama(c)
                | StrategyConfig::LinReg(c)
                | StrategyConfig::McGinley(c)
                | StrategyConfig::Pwma(c)
                | StrategyConfig::SinWma(c)
                | StrategyConfig::Sma(c)
                | StrategyConfig::SmaCd(c)
                | StrategyConfig::Smma(c)
                | StrategyConfig::Swma(c)
                | StrategyConfig::Tema(c)
                | StrategyConfig::Tilson(c)
                | StrategyConfig::Trima(c)
                | StrategyConfig::Vidya(c)
                | StrategyConfig::Vwma(c)
                | StrategyConfig::Wilders(c)
                | StrategyConfig::Wma(c)
                | StrategyConfig::Zlma(c) => c.validate()?,
                StrategyConfig::MabbLong(c) => c.validate()?,
                StrategyConfig::MabbShort(c) => c.validate()?,
                StrategyConfig::Macd(c) => c.validate()?,
                StrategyConfig::RBreaker(c) => c.validate()?,
                StrategyConfig::RSI2(c) => c.validate()?,
                StrategyConfig::Sgma(c) => c.validate()?,
                StrategyConfig::SsfCd(c) => c.validate()?,
                StrategyConfig::Ssf(c) => c.validate()?,
                StrategyConfig::SsfStCd(c) => c.validate()?,
                StrategyConfig::SsfSt(c) => c.validate()?,
                StrategyConfig::Turtle(c) => c.validate()?,
                StrategyConfig::Vwap(c) => c.validate()?,
                StrategyConfig::VwapTrend(c) => c.validate()?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacktestConfig {
    pub initial_balance: f64,
    /// 信号到成交的延迟(bar 数)。只允许≥1: 信号次根 fill bar 成交;
    /// delay = 0(信号当根收盘成交, 前视乐观)已删除, 写0直接报错。
    #[serde(default = "default_delay_bars")]
    pub delay_bars: usize,
    /// 单边佣金(bp)。如 taker 2bp 填 2.0, maker 零费填 0。
    #[serde(default)]
    pub fee_bps: f64,
    /// 单边逆向滑点(bp)。买入抬价、卖出压价; 与 fee 叠加扣除。
    #[serde(default)]
    pub slippage_bps: f64,
    /// fill bar 的成交价: "open"(默认, 次根开盘)或 "close"(次根收盘)。
    /// 旧配置文件无此键时回退为 open, 历史结果不受影响。
    #[serde(default)]
    pub fill_bar_price: FillBarPrice,
}

/// 缺省延迟 1 bar (次根 fill bar 成交); delay = 0 已删除, 见 validate。
fn default_delay_bars() -> usize {
    1
}

/// fill bar 的成交价位 (delay恒≥1)。
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
    /// 是否写 fixedcurve 文件（`--pbo` 的 T×N 输入）。默认 false：
    /// detail 只落逐笔 trades，省掉 GB 级曲线 I/O；做 PBO 时显式开 true。
    #[serde(default)]
    pub fixed_curves: bool,
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

impl MaGridCfg {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
}

impl SsfStConfig {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
}

impl SsfStCdConfig {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
}

impl SsfCdConfig {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
}

impl SsfConfig {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
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
    Smma(MaGridCfg),
    Ehma(MaGridCfg),
    Frama(MaGridCfg),
    Dma(MaGridCfg),
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
    Edma(EdmaConfig),
    Atma(AtmaConfig),
    Sgma(SgmaConfig),
    #[serde(rename = "atr_band")]
    ATRBand(ATRBandConfig),
    Keltner(KeltnerConfig),
    #[serde(rename = "bbmr")]
    BBMR(BBMRConfig),
    Bbrk(BBRKConfig),
    BbrkAdx(BbrkAdxConfig),
    BbrkTrail(BbrkTrailConfig),
    BbrkDon(BbrkDonConfig),
    BbrkEma(BbrkEmaConfig),
    BbrkPctb(BbrkPctbConfig),
    BbrkAtr(BbrkAtrConfig),
    BbrkStr(BbrkStrConfig),
    BbrkSma(BbrkSmaConfig),
    BbrkHma(BbrkHmaConfig),
    BbrkMax(BbrkMaxConfig),
    BbrkBb(BbrkBbConfig),
    BbrkVolEma(BbrkVolEmaConfig),
    BbrkAtrEma(BbrkAtrEmaConfig),
    BbrkSmaEma(BbrkSmaEmaConfig),
    BbrkHmaEma(BbrkHmaEmaConfig),
    BbrkHmaV1(BbrkHmaV1Config),
    BbrkHmaV2(BbrkHmaV2Config),
    BbrkHmaV3(BbrkHmaV3Config),
    BbrkHmaV4(BbrkHmaV4Config),
    BbrkHmaV5(BbrkHmaV5Config),
    BbrkHmaV1Sl(BbrkHmaV1SlConfig),
    BbrkHmaV2Sl(BbrkHmaV2SlConfig),
    BbrkHmaV3Sl(BbrkHmaV3SlConfig),
    BbrkHmaV4Sl(BbrkHmaV4SlConfig),
    BbrkHmaV5Sl(BbrkHmaV5SlConfig),
    BbrkZlemaEma(BbrkZlemaEmaConfig),
    BbrkRex(BbrkRexConfig),
    BbrkVol(BbrkVolConfig),
    BbrkTtm(BbrkTtmConfig),
    BbrkTtmFire(BbrkTtmFireConfig),
    BbrkCross(BbrkCrossConfig),
    BbrkKdj(BbrkKdjConfig),
    BbrkM2(BbrkM2Config),
    BbrkM3(BbrkM3Config),
    BbrkOrth(BbrkOrthConfig),
    VwapTrend(VwapTrendConfig),
    #[serde(rename = "rsi2")]
    RSI2(RSI2Config),
    Vwap(VwapConfig),
    MabbLong(MabbLongConfig),
    MabbShort(MabbShortConfig),
    Macd(MacdConfig),
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
    fn validate_rejects_zero_delay_bars() {
        let toml = make_toml(10000.0, true, true).replace(
            "initial_balance = 10000",
            "initial_balance = 10000.0\ndelay_bars = 0",
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn missing_delay_bars_defaults_to_one() {
        let cfg: Config = toml::from_str(&make_toml(10000.0, true, true)).unwrap();
        assert_eq!(cfg.backtest.delay_bars, 1);
        assert!(cfg.validate().is_ok());
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
            "type = \"bbrk_adx\"\nsigma = { start = 2.0, end = 3.0, step = 0.5 }\nadx = { start = 20.0, end = 25.0, step = 5.0 }",
            "type = \"bbrk_trail\"\nsigma = { start = 2.0, end = 3.0, step = 0.5 }\ntrailk = { start = 2.0, end = 3.0, step = 1.0 }",
            "type = \"bbrk_don\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nexitn = { start = 2.0, end = 3.0, step = 1.0 }",
            "type = \"bbrk_pctb\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nexitb = { start = 0.2, end = 0.5, step = 0.3 }",
            "type = \"bbrk_atr\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\natrthr = { start = 0.8, end = 1.0, step = 0.2 }",
            "type = \"bbrk_str\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nbody = { start = 0.3, end = 0.5, step = 0.2 }",
            "type = \"bbrk_sma\"\nsigma = 2.5\nsmareg = 100.0",
            "type = \"bbrk_hma\"\nsigma = 2.5\nhmalen = 100.0",
            "type = \"bbrk_max\"\nma = [\"sma\", \"hma\"]\nmalen = 100.0",
            "type = \"bbrk_bb\"\nma = [\"sma\"]\nbblen = 20.0\nsigma = 2.5",
            "type = \"bbrk_vol_ema\"\nsigma = 2.5\nvoln = 10.0\nvolmult = 1.5",
            "type = \"bbrk_atr_ema\"\nsigma = 2.5\natrthr = 1.0",
            "type = \"bbrk_sma_ema\"\nsigma = 2.5\nsmareg = 100.0",
            "type = \"bbrk_hma_ema\"\nsigma = 2.5\nhmalen = 100.0",
            "type = \"bbrk_hma_v1\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0",
            "type = \"bbrk_hma_v2\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0",
            "type = \"bbrk_hma_v3\"\nsigma = 2.5\nbblen = 20.0",
            "type = \"bbrk_hma_v4\"\nsigma = 2.5\nbblen = 20.0",
            "type = \"bbrk_hma_v5\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0",
            "type = \"bbrk_hma_v1_sl\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0\nstopk = 2.0\natrlen = 50.0",
            "type = \"bbrk_hma_v2_sl\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0\nstopk = 2.0\natrlen = 50.0",
            "type = \"bbrk_hma_v3_sl\"\nsigma = 2.5\nbblen = 20.0\nstopk = 2.0\natrlen = 50.0",
            "type = \"bbrk_hma_v4_sl\"\nsigma = 2.5\nbblen = 20.0\nstopk = 2.0\natrlen = 50.0",
            "type = \"bbrk_hma_v5_sl\"\nsigma = 2.5\nbblen = 20.0\nhmalen = 450.0\nstopk = 2.0\natrlen = 50.0",
            "type = \"mabb_long\"\nma = [\"sma\"]\nbblen = 20.0\nsigma = 2.5\nstopk = 0.0\natrlen = 14.0",
            "type = \"mabb_short\"\nma = [\"ema\"]\nbblen = 20.0\nsigma = 2.5\nstopk = 2.0\natrlen = 14.0",
            "type = \"bbrk_zlema_ema\"\nsigma = 2.5\nzlemareg = 100.0",
            "type = \"bbrk_rex\"\nsigma = { start = 2.0, end = 2.5, step = 0.5 }\nrexn = { start = 3.0, end = 5.0, step = 2.0 }\nrexmult = 1.5",
            "type = \"bbrk_vol\"\nsigma = 2.5\nvoln = 10.0\nvolmult = 1.5",
            "type = \"bbrk_ttm\"\nsigma = 2.5\nkc = 1.5\nttmn = 5.0",
            "type = \"bbrk_ttm_fire\"\nsigma = 2.5\nkc = 1.5",
            "type = \"vwap_trend\"\nreset = { start = 0.0, end = 1.0, step = 1.0 }",
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
    fn for_pair_produces_single_value_config_for_kline_reader() {
        let cfg: Config = toml::from_str(&make_scan_toml(
            "[\"BTCUSDT\", \"ETHUSDT\"]",
            "[\"15m\", \"1h\"]",
        ))
        .unwrap();
        let pair = cfg.data.for_pair("ETHUSDT", "1h").unwrap();
        assert_eq!(pair.symbol.as_str(), "ETHUSDT");
        assert_eq!(pair.timeframe.as_str(), "1h");
        assert_eq!(pair.dir, cfg.data.dir);
        assert_eq!(pair.source_type, cfg.data.source_type);
    }

    #[test]
    fn for_pair_preserves_date_window() {
        let toml = "[data]\ntype = \"binance\"\ndir = \"/tmp\"\nsymbol = [\"BTCUSDT\"]\ntimeframe = [\"15m\"]\nstart_date = \"2024-01-01\"\nend_date = \"2024-06-30\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"ssf_st\"\nperiod = 3\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let pair = cfg.data.for_pair("BTCUSDT", "15m").unwrap();
        assert_eq!(
            pair.start_date.map(|d| d.to_string()).as_deref(),
            Some("2024-01-01")
        );
        assert_eq!(
            pair.end_date.map(|d| d.to_string()).as_deref(),
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
}
