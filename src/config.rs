use std::num::NonZeroUsize;
use std::path::PathBuf;

use serde::Deserialize;

use crate::engine::StopConfig;
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
use crate::strategies::bbrk_hma_v2::BbrkHmaV2Config;
use crate::strategies::bbrk_hma_v3::BbrkHmaV3Config;
use crate::strategies::bbrk_hma_v4::BbrkHmaV4Config;
use crate::strategies::bbrk_hma_v5::BbrkHmaV5Config;
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
use crate::strategies::ma_atr::MaAtrConfig;
use crate::strategies::ma_slope_atr_gate::MaSlopeAtrGateConfig;
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
use crate::strategies::swing_fractal::SwingFractalConfig;
use crate::strategies::turtle::TurtleConfig;
use crate::strategies::vwap::VwapConfig;
use crate::strategies::vwap_trend::VwapTrendConfig;

/// 一条 `[[strategy]]` 条目: 策略参数 + **该策略专属**的持仓期止损。
///
/// 用 `#[serde(flatten)]` 而不是嵌套 `strategy = { ... }`: TOML 里保持
/// `type = "sma"` 平铺, 同时允许 `stop = { stop_loss_pct = 2.0 }` 或
/// `[strategy.stop]` 两种写法。`stop` 缺省为“无止损”。
///
/// 同一策略可以出现多次, 每次配不同的 `stop`, 从而形成 stop 参数网格;
/// `params` 会带止损后缀, 缓存指纹与报告行因此保持唯一。
#[derive(Debug, Clone, Deserialize)]
pub struct StrategySpec {
    /// 策略自身的配置(`type` + 参数)。
    #[serde(flatten)]
    pub strategy: StrategyConfig,
    /// 该策略的持仓期止损; 缺省无止损。
    #[serde(default)]
    pub stop: StopConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// 回测配置根。`deny_unknown_fields`: 拼错的键会直接报错而非被忽略。
pub struct Config {
    /// 数据源与扫描范围(品种 × 周期)。
    pub data: DataSection,
    /// 执行口径(资金 / 延迟 / 费率)。
    pub backtest: BacktestConfig,
    /// 输出选项; 缺省写到 `./reports/`。
    pub output: Option<OutputConfig>,
    /// 要跑的策略列表(至少一个); 每条可带独立止损。
    pub strategy: Vec<StrategySpec>,
}

/// 单值或列表, 统一归一化为 `Vec<String>`。
/// TOML 里 `symbol = "BTCUSDT"` 与 `symbol = ["BTCUSDT", "ETHUSDT"]` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 单值或列表, 统一归一化。TOML 里 `symbol = "BTC"` 与 `symbol = ["BTC","ETH"]` 都合法。
pub enum OneOrMany {
    /// 单值形态。
    One(String),
    /// 列表形态。
    Many(Vec<String>),
}

impl OneOrMany {
    /// 归一化后的品种/周期列表(**借用**)。
    ///
    /// 收 `&self` 返回 `&[String]` 而不是克隆一份 `Vec<String>`: 调用方
    /// (`Config::validate` / `app::run`) 每次都要用这个列表, 而克隆会让
    /// 每次调用多分配一条与列表等长的向量。
    #[must_use]
    pub fn items(&self) -> &[String] {
        match self {
            OneOrMany::One(s) => std::slice::from_ref(s),
            OneOrMany::Many(v) => v.as_slice(),
        }
    }
}

/// vta 自己的数据配置: 与 `kline_reader::DataConfig` 字段一致, 但 symbol/timeframe 支持列表。
/// 加载时通过 [`DataSection::for_pair`] 降为 kline-reader 需要的单值形态。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSection {
    /// 数据源类型(如 `binance`)。
    #[serde(rename = "type")]
    pub source_type: kline_reader::DataSource,
    /// Binance 市场(spot/futures/um/futures/cm), 缺省 `futures/um`。
    #[serde(default)]
    pub market: Option<kline_reader::Market>,
    /// 数据根目录。
    pub dir: PathBuf,
    /// 品种, 单值或列表。
    pub symbol: OneOrMany,
    /// 周期, 单值或列表。
    pub timeframe: OneOrMany,
    #[serde(default)]
    /// 起始日期(含), 缺省不限。
    pub start_date: Option<String>,
    #[serde(default)]
    /// 结束日期(含), 缺省不限。
    pub end_date: Option<String>,
}

impl DataSection {
    /// 品种列表。
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        self.symbol.items()
    }

    /// 周期列表。
    #[must_use]
    pub fn timeframes(&self) -> &[String] {
        self.timeframe.items()
    }

    /// 取出一组 (symbol, timeframe) 对应的单值配置, 供 `kline_reader::load_columns` 使用。
    /// 已验证构造：非法 symbol/timeframe 即 `Err`，调用方按跳过处理（不 `unwrap`）。
    ///
    /// # Errors
    /// `symbol` / `timeframe` 不合法时返回
    /// [`kline_reader::ReaderError`]; 调用方按"跳过该 (品种, 周期) 对"处理。
    pub fn for_pair(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> Result<kline_reader::DataConfig, kline_reader::ReaderError> {
        match self.market {
            Some(market) => kline_reader::DataConfig::new_with_market(
                self.source_type,
                market,
                &self.dir,
                symbol,
                timeframe,
                self.start_date.as_deref(),
                self.end_date.as_deref(),
            ),
            None => kline_reader::DataConfig::new(
                self.source_type,
                &self.dir,
                symbol,
                timeframe,
                self.start_date.as_deref(),
                self.end_date.as_deref(),
            ),
        }
    }
}

impl Config {
    /// 语义校验。**执行口径(资金/费率/延迟)已由 [`BacktestConfig`] 的字段类型
    /// 在反序列化时保证**, 此处不再重复检查; 只剩数据维度与策略专属约束。
    /// 语义校验。
    ///
    /// # Errors
    /// 品种/周期为空、**品种/周期含路径穿越字符**、周期未知或策略参数非法时
    /// 返回 [`AppError`]。
    pub fn validate(&self) -> crate::error::Result<()> {
        let syms = self.data.symbols();
        if syms.is_empty() || syms.iter().any(String::is_empty) {
            return Err(AppError::Msg("data.symbol 不能为空".into()));
        }
        // 品种名会**直接进入落盘文件名**(`{symbol}_{tf}_{strategy}_…parquet`),
        // 故必须在这里挡住路径穿越 —— 否则 `symbol = "../evil"` 会让产物写到
        // 输出目录的**上一层**。真实品种名都是字母数字(`BTCUSDT` /
        // `1000SHIBUSDT` / `1INCHUSDT`), 白名单不会误伤。
        for s in syms {
            check_path_safe_component("data.symbol", s)?;
        }
        let tfs = self.data.timeframes();
        if tfs.is_empty() || tfs.iter().any(String::is_empty) {
            return Err(AppError::Msg("data.timeframe 不能为空".into()));
        }
        for tf in tfs {
            // 周期同理: 它也进文件名(`{symbol}_{tf}_…`)。
            check_path_safe_component("data.timeframe", tf)?;
            if !crate::strategies::is_known_timeframe(tf) {
                return Err(AppError::Msg(format!(
                    "未知周期 data.timeframe = {tf:?}; 已知: 1m/3m/5m/15m/30m/1h/2h/4h/6h/8h/12h/1d/1w"
                )));
            }
        }
        // `fixed_curves` 只在 detail 分支写出。若允许它单独为 true, 用户会得到
        // "配置说要做 PBO, 但 detail/ 一个 fixedcurve 都没有" 的静默空结果 ——
        // 随后 `--pbo` 报"未找到曲线", 排查方向完全错。这里直接拒绝。
        if self
            .output
            .as_ref()
            .is_some_and(|output| output.fixed_curves && !output.detail)
        {
            return Err(AppError::Msg(
                "output.fixed_curves = true 需要同时设置 output.detail = true: \
                 fixedcurve 只在 detail 分支写出"
                    .into(),
            ));
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
        for spec in &self.strategy {
            spec.stop.validate()?;
            match &spec.strategy {
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
                StrategyConfig::BbrkHmaV2(c) => c.validate()?,
                StrategyConfig::BbrkHmaV3(c) => c.validate()?,
                StrategyConfig::BbrkHmaV4(c) => c.validate()?,
                StrategyConfig::BbrkHmaV5(c) => c.validate()?,
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
                StrategyConfig::MaAtr(c) => c.validate()?,
                StrategyConfig::MaSlopeAtrGate(c) => c.validate()?,
                StrategyConfig::Macd(c) => c.validate()?,
                StrategyConfig::RBreaker(c) => c.validate()?,
                StrategyConfig::RSI2(c) => c.validate()?,
                StrategyConfig::Sgma(c) => c.validate()?,
                StrategyConfig::SsfCd(c) => c.validate()?,
                StrategyConfig::Ssf(c) => c.validate()?,
                StrategyConfig::SsfStCd(c) => c.validate()?,
                StrategyConfig::SsfSt(c) => c.validate()?,
                StrategyConfig::SwingFractal(c) => c.validate()?,
                StrategyConfig::Turtle(c) => c.validate()?,
                StrategyConfig::Vwap(c) => c.validate()?,
                StrategyConfig::VwapTrend(c) => c.validate()?,
            }
        }
        Ok(())
    }
}

/// 初始资金: **严格正**, 且是**定点十进制金额**。
///
/// 用 newtype 而非裸 `f64` + 运行时 `if`: 后者让 `balance = 0` 成为可表示的状态,
/// 而它在下游会触发 `qty = cash / entry_price` 与 `fixed_qty = initial_cash / entry`
/// 的除零。现在该值**无法用非法状态表示**。
///
/// 内部用 [`Amount`](crate::money::Amount)(定点十进制)而非 `f64` ——
/// 资金是**货币**, `domain-fintech` 的硬规则是"Never use `f64` for money"。
/// 此前本 newtype 只是给 `f64` 套了个名字: `get()` 之后 `balance: f64` 一路流通,
/// 任何人都能直接构造 `f64` 绕过校验 —— 那是**假**的类型安全。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct InitialBalance(crate::money::Amount);

impl InitialBalance {
    /// 金额值(定点十进制)。
    #[must_use]
    pub fn amount(self) -> crate::money::Amount {
        self.0
    }

    /// 取回 `f64` 形式(**仅供统计/落盘口径**, 内部金额运算不得用)。
    #[must_use]
    pub fn get(self) -> f64 {
        self.0.to_f64()
    }
}

impl TryFrom<f64> for InitialBalance {
    type Error = AppError;

    fn try_from(v: f64) -> crate::error::Result<Self> {
        if v > 0.0 && v.is_finite() {
            Ok(Self(crate::money::Amount::from_f64(v)?))
        } else {
            Err(AppError::Msg(format!(
                "backtest.initial_balance 必须 > 0 且为有限值, 当前为 {v}"
            )))
        }
    }
}

/// 单边费率(bp): **非负且有限**。负费率(返佣)当前未被任何语义支持, 故一并拒绝。
macro_rules! nonneg_bps {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        /// 单边费率(bp): **非负且有限**。见模块内 `nonneg_bps` 宏的说明。
        pub struct $name(f64);

        impl $name {
            /// 取回原始数值。
            #[must_use]
            pub fn get(self) -> f64 {
                self.0
            }
        }

        impl TryFrom<f64> for $name {
            type Error = AppError;

            fn try_from(v: f64) -> crate::error::Result<Self> {
                if v >= 0.0 && v.is_finite() {
                    Ok(Self(v))
                } else {
                    Err(AppError::Msg(format!(
                        "backtest.{} 不能为负且须为有限值, 当前为 {v}",
                        $label
                    )))
                }
            }
        }
    };
}
nonneg_bps!(FeeBps, "fee_bps");
nonneg_bps!(SlippageBps, "slippage_bps");

/// 缺省延迟 1 bar (次根 fill bar 成交)。`delay_bars = 0` 由 [`NonZeroUsize`]
/// 在构造期拒绝。
fn default_delay_bars() -> usize {
    1
}

/// 反序列化中间层: 全部字段为裸类型 + serde `default`。
/// 只做**语法**解析(缺省填充), 语义合法性交给 [`TryFrom`]。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBacktestConfig {
    initial_balance: f64,
    #[serde(default = "default_delay_bars")]
    delay_bars: usize,
    #[serde(default)]
    fee_bps: f64,
    #[serde(default)]
    slippage_bps: f64,
    #[serde(default)]
    fill_bar_price: FillBarPrice,
}

/// 回测执行配置。**字段类型保证值合法**, 无需事后 `validate()`:
/// `try_from` 使非法 TOML 在**反序列化时**就报错(自带行列号), 而不是在 `run()`
/// 里被逐条 `if` 拦下。
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawBacktestConfig")]
pub struct BacktestConfig {
    /// 初始资金: 严格正且有限。
    pub initial_balance: InitialBalance,
    /// 信号到成交的 bar 间隔, 恒 ≥ 1。`delay_bars = 0`(信号当根成交, 前视乐观)
    /// 已在引擎层彻底移除, 此处类型级拒绝。
    pub delay_bars: NonZeroUsize,
    /// 单边佣金(bp): 非负且有限。
    pub fee_bps: FeeBps,
    /// 单边逆向滑点(bp): 非负且有限。
    pub slippage_bps: SlippageBps,
    /// fill bar 的成交价: "open"(默认, 次根开盘)或 "close"(次根收盘)。
    pub fill_bar_price: FillBarPrice,
}

impl TryFrom<RawBacktestConfig> for BacktestConfig {
    type Error = AppError;

    fn try_from(r: RawBacktestConfig) -> crate::error::Result<Self> {
        Ok(Self {
            initial_balance: InitialBalance::try_from(r.initial_balance)?,
            delay_bars: NonZeroUsize::new(r.delay_bars).ok_or_else(|| {
                AppError::Msg(
                    "backtest.delay_bars 必须 >= 1: 信号当根成交为前视乐观口径, 已移除".to_string(),
                )
            })?,
            fee_bps: FeeBps::try_from(r.fee_bps)?,
            slippage_bps: SlippageBps::try_from(r.slippage_bps)?,
            fill_bar_price: r.fill_bar_price,
        })
    }
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
    /// 输出根目录; 缺省 `./reports/`。
    pub dir: Option<PathBuf>,
    /// 是否导出逐笔交易和权益曲线详细数据
    #[serde(default)]
    pub detail: bool,
    /// 是否写 fixedcurve 文件（`--pbo` 的 T×N 输入）。默认 false：
    /// detail 仍会写逐笔 trades 与权益曲线，只是不写 fixed 曲线；
    /// 做 PBO 时显式开 true（且必须同时 `detail = true`）。
    #[serde(default)]
    pub fixed_curves: bool,
    /// 是否覆盖已存在的回测结果文件，默认 false（跳过已完成的组合）
    #[serde(default)]
    pub overwrite: bool,
}

/// 校验一个会**进入落盘文件名**的字符串是安全的路径片段。
///
/// 拒绝:
/// - 任何路径分隔符(`/`、`\`)、盘符分隔符(`:`);
/// - `..`(父目录)与 `.`(当前目录)段;
/// - 其它控制字符与空白(它们在文件名里毫无意义, 还会让跨平台行为不一致);
/// - 空串。
///
/// 允许: 字母、数字、`.`、`-`、`_`、`+` —— 覆盖全部真实品种名
/// (`BTCUSDT` / `1000SHIBUSDT` / `1INCHUSDT` / `BRK-B`) 与周期名(`1m` / `1d`)。
///
/// # Errors
/// 不安全时返回 [`AppError`]。
fn check_path_safe_component(what: &str, value: &str) -> crate::error::Result<()> {
    if value.is_empty() {
        return Err(AppError::Msg(format!("{what} 不能为空")));
    }
    if value == "." || value == ".." {
        return Err(AppError::Msg(format!(
            "{what} = {value:?} 是路径片段, 不得直接进入文件名"
        )));
    }
    for c in value.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+');
        if !ok {
            return Err(AppError::Msg(format!(
                "{what} = {value:?} 含非法字符 {c:?}; \
                 它会直接进入落盘文件名, 只允许字母数字与 `.` `-` `_` `+`"
            )));
        }
    }
    Ok(())
}

/// Config for single-MA price-cross strategies.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaGridCfg {
    /// 均线周期, 单值或范围。
    pub period: PeriodParam,
}

impl MaGridCfg {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    /// 语义校验: 只需校验 `period` 的区间与方向。
    ///
    /// # Errors
    /// `period` 越界时返回 [`AppError`]。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()
    }
}

/// 策略配置。以 TOML 的 `type` 标签做**内部标签**判别。
///
/// `deny_unknown_fields`: 拼错的键直接报错, 不会被静默忽略。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StrategyConfig {
    /// **Sma** 均线策略: 配置 [`MaGridCfg`]。
    Sma(MaGridCfg),
    /// **SmaCd** 均线策略: 配置 [`MaGridCfg`]。
    SmaCd(MaGridCfg),
    /// **Ema** 均线策略: 配置 [`MaGridCfg`]。
    Ema(MaGridCfg),
    /// **Hma** 均线策略: 配置 [`MaGridCfg`]。
    Hma(MaGridCfg),
    /// **Zlma** 均线策略: 配置 [`MaGridCfg`]。
    Zlma(MaGridCfg),
    /// **Alma** 均线策略: 配置 [`MaGridCfg`]。
    Alma(MaGridCfg),
    /// **Dema** 均线策略: 配置 [`MaGridCfg`]。
    Dema(MaGridCfg),
    /// **Fwma** 均线策略: 配置 [`MaGridCfg`]。
    Fwma(MaGridCfg),
    /// **Jma** 均线策略: 配置 [`MaGridCfg`]。
    Jma(MaGridCfg),
    /// **Kama** 均线策略: 配置 [`MaGridCfg`]。
    Kama(MaGridCfg),
    /// **LinReg** 均线策略: 配置 [`MaGridCfg`]。
    LinReg(MaGridCfg),
    /// **Pwma** 均线策略: 配置 [`MaGridCfg`]。
    Pwma(MaGridCfg),
    /// **SinWma** 均线策略: 配置 [`MaGridCfg`]。
    SinWma(MaGridCfg),
    /// **Swma** 均线策略: 配置 [`MaGridCfg`]。
    Swma(MaGridCfg),
    /// **Tema** 均线策略: 配置 [`MaGridCfg`]。
    Tema(MaGridCfg),
    /// **Tilson** 均线策略: 配置 [`MaGridCfg`]。
    Tilson(MaGridCfg),
    /// **Trima** 均线策略: 配置 [`MaGridCfg`]。
    Trima(MaGridCfg),
    /// **Wilders** 均线策略: 配置 [`MaGridCfg`]。
    Wilders(MaGridCfg),
    /// **Wma** 均线策略: 配置 [`MaGridCfg`]。
    Wma(MaGridCfg),
    /// **Smma** 均线策略: 配置 [`MaGridCfg`]。
    Smma(MaGridCfg),
    /// **Ehma** 均线策略: 配置 [`MaGridCfg`]。
    Ehma(MaGridCfg),
    /// **Frama** 均线策略: 配置 [`MaGridCfg`]。
    Frama(MaGridCfg),
    /// **Dma** 均线策略: 配置 [`MaGridCfg`]。
    Dma(MaGridCfg),
    /// **Vwma** 均线策略: 配置 [`MaGridCfg`]。
    Vwma(MaGridCfg),
    /// **Vidya** 均线策略: 配置 [`MaGridCfg`]。
    Vidya(MaGridCfg),
    /// **McGinley** 均线策略: 配置 [`MaGridCfg`]。
    McGinley(MaGridCfg),
    /// **Ssf** 策略: 配置 [`SsfConfig`]。
    Ssf(SsfConfig),
    /// **SsfCd** 策略: 配置 [`SsfCdConfig`]。
    SsfCd(SsfCdConfig),
    /// **SsfSt** 策略: 配置 [`SsfStConfig`]。
    SsfSt(SsfStConfig),
    /// **SsfStCd** 策略: 配置 [`SsfStCdConfig`]。
    SsfStCd(SsfStCdConfig),
    /// **SwingFractal** 策略: 配置 [`SwingFractalConfig`]。 TOML 中写作 `type = "swing_fractal"`。
    SwingFractal(SwingFractalConfig),
    /// **Turtle** 策略: 配置 [`TurtleConfig`]。
    Turtle(TurtleConfig),
    /// **DualThrust** 策略: 配置 [`DualThrustConfig`]。
    DualThrust(DualThrustConfig),
    /// **RBreaker** 策略: 配置 [`RBreakerConfig`]。
    RBreaker(RBreakerConfig),
    /// **Fiali** 策略: 配置 [`FialiConfig`]。
    Fiali(FialiConfig),
    /// **Aberration** 策略: 配置 [`AberrationConfig`]。
    Aberration(AberrationConfig),
    /// **Edma** 策略: 配置 [`EdmaConfig`]。
    Edma(EdmaConfig),
    /// **Atma** 策略: 配置 [`AtmaConfig`]。
    Atma(AtmaConfig),
    /// **Sgma** 策略: 配置 [`SgmaConfig`]。
    Sgma(SgmaConfig),
    #[serde(rename = "atr_band")]
    /// **ATRBand** 策略: 配置 [`ATRBandConfig`]。 TOML 中写作 `type = "atr_band"`。
    ATRBand(ATRBandConfig),
    /// **Keltner** 策略: 配置 [`KeltnerConfig`]。
    Keltner(KeltnerConfig),
    #[serde(rename = "bbmr")]
    /// **BBMR** 策略: 配置 [`BBMRConfig`]。 TOML 中写作 `type = "bbmr"`。
    BBMR(BBMRConfig),
    /// **Bbrk** 策略: 配置 [`BBRKConfig`]。
    Bbrk(BBRKConfig),
    /// **BbrkAdx** 策略: 配置 [`BbrkAdxConfig`]。
    BbrkAdx(BbrkAdxConfig),
    /// **BbrkTrail** 策略: 配置 [`BbrkTrailConfig`]。
    BbrkTrail(BbrkTrailConfig),
    /// **BbrkDon** 策略: 配置 [`BbrkDonConfig`]。
    BbrkDon(BbrkDonConfig),
    /// **BbrkEma** 策略: 配置 [`BbrkEmaConfig`]。
    BbrkEma(BbrkEmaConfig),
    /// **BbrkPctb** 策略: 配置 [`BbrkPctbConfig`]。
    BbrkPctb(BbrkPctbConfig),
    /// **BbrkAtr** 策略: 配置 [`BbrkAtrConfig`]。
    BbrkAtr(BbrkAtrConfig),
    /// **BbrkStr** 策略: 配置 [`BbrkStrConfig`]。
    BbrkStr(BbrkStrConfig),
    /// **BbrkSma** 策略: 配置 [`BbrkSmaConfig`]。
    BbrkSma(BbrkSmaConfig),
    /// **BbrkHma** 策略: 配置 [`BbrkHmaConfig`]。
    BbrkHma(BbrkHmaConfig),
    /// **BbrkMax** 策略: 配置 [`BbrkMaxConfig`]。
    BbrkMax(BbrkMaxConfig),
    /// **BbrkBb** 策略: 配置 [`BbrkBbConfig`]。
    BbrkBb(BbrkBbConfig),
    /// **BbrkVolEma** 策略: 配置 [`BbrkVolEmaConfig`]。
    BbrkVolEma(BbrkVolEmaConfig),
    /// **BbrkAtrEma** 策略: 配置 [`BbrkAtrEmaConfig`]。
    BbrkAtrEma(BbrkAtrEmaConfig),
    /// **BbrkSmaEma** 策略: 配置 [`BbrkSmaEmaConfig`]。
    BbrkSmaEma(BbrkSmaEmaConfig),
    /// **BbrkHmaEma** 策略: 配置 [`BbrkHmaEmaConfig`]。
    BbrkHmaEma(BbrkHmaEmaConfig),
    /// **BbrkHmaV1** 策略: 配置 [`BbrkHmaV1Config`]。
    BbrkHmaV1(BbrkHmaV1Config),
    /// **BbrkHmaV2** 策略: 配置 [`BbrkHmaV2Config`]。
    BbrkHmaV2(BbrkHmaV2Config),
    /// **BbrkHmaV3** 策略: 配置 [`BbrkHmaV3Config`]。
    BbrkHmaV3(BbrkHmaV3Config),
    /// **BbrkHmaV4** 策略: 配置 [`BbrkHmaV4Config`]。
    BbrkHmaV4(BbrkHmaV4Config),
    /// **BbrkHmaV5** 策略: 配置 [`BbrkHmaV5Config`]。
    BbrkHmaV5(BbrkHmaV5Config),
    /// **BbrkZlemaEma** 策略: 配置 [`BbrkZlemaEmaConfig`]。
    BbrkZlemaEma(BbrkZlemaEmaConfig),
    /// **BbrkRex** 策略: 配置 [`BbrkRexConfig`]。
    BbrkRex(BbrkRexConfig),
    /// **BbrkVol** 策略: 配置 [`BbrkVolConfig`]。
    BbrkVol(BbrkVolConfig),
    /// **BbrkTtm** 策略: 配置 [`BbrkTtmConfig`]。
    BbrkTtm(BbrkTtmConfig),
    /// **BbrkTtmFire** 策略: 配置 [`BbrkTtmFireConfig`]。
    BbrkTtmFire(BbrkTtmFireConfig),
    /// **BbrkCross** 策略: 配置 [`BbrkCrossConfig`]。
    BbrkCross(BbrkCrossConfig),
    /// **BbrkKdj** 策略: 配置 [`BbrkKdjConfig`]。
    BbrkKdj(BbrkKdjConfig),
    /// **BbrkM2** 策略: 配置 [`BbrkM2Config`]。
    BbrkM2(BbrkM2Config),
    /// **BbrkM3** 策略: 配置 [`BbrkM3Config`]。
    BbrkM3(BbrkM3Config),
    /// **BbrkOrth** 策略: 配置 [`BbrkOrthConfig`]。
    BbrkOrth(BbrkOrthConfig),
    /// **VwapTrend** 策略: 配置 [`VwapTrendConfig`]。
    VwapTrend(VwapTrendConfig),
    #[serde(rename = "rsi2")]
    /// **RSI2** 策略: 配置 [`RSI2Config`]。 TOML 中写作 `type = "rsi2"`。
    RSI2(RSI2Config),
    /// **Vwap** 策略: 配置 [`VwapConfig`]。
    Vwap(VwapConfig),
    /// **MabbLong** 策略: 配置 [`MabbLongConfig`]。
    MabbLong(MabbLongConfig),
    /// **MabbShort** 策略: 配置 [`MabbShortConfig`]。
    MabbShort(MabbShortConfig),
    /// **MaAtr** 策略: 配置 [`MaAtrConfig`]。
    MaAtr(MaAtrConfig),
    /// **MaSlopeAtrGate** 策略: 配置 [`MaSlopeAtrGateConfig`]。
    MaSlopeAtrGate(MaSlopeAtrGateConfig),
    /// **Macd** 策略: 配置 [`MacdConfig`]。
    Macd(MacdConfig),
    /// **EmaCross** 策略: 配置 [`EmaCrossConfig`]。
    EmaCross(EmaCrossConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 整型参数: 单值或范围。
pub enum PeriodParam {
    /// 单值形态。
    Single(usize),
    /// 范围形态 `{ start, end, step }`。
    Range(RangeParam<usize>),
}

impl PeriodParam {
    /// 语义校验: 范围必须 `start >= 1`、`start <= end`、`step > 0`。
    ///
    /// # Errors
    /// 任一约束不满足时返回 [`AppError`]。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.validate_named("period", 1)
    }

    /// 语义校验, **参数名可定制**且允许非 1 的下界。
    ///
    /// 为什么需要它: 策略配置里除 `period` 外还有"步长型"整型参数
    /// (`sgma.poly_order` 多项式阶数、`edma.offset` 偏移量), 它们的合法下界
    /// 与字段名都和 `period` 不同。用 `validate()` 一刀切会误拒合法值
    /// (例如 `offset = 0`), 而**不校验**又会漏掉 `step = 0` ——
    /// 后者会在展开网格时触发 `Range::step_by(0)` 的 panic。
    ///
    /// # Errors
    /// 任一约束不满足时返回 [`AppError`], 消息里带 `label` 便于定位。
    pub fn validate_named(&self, label: &str, min: usize) -> crate::error::Result<()> {
        match self {
            PeriodParam::Single(v) => {
                if *v < min {
                    return Err(AppError::Msg(format!("{label} 必须 >= {min}, 当前为 {v}")));
                }
            }
            PeriodParam::Range(r) => {
                if r.start < min {
                    return Err(AppError::Msg(format!(
                        "{label} 必须 >= {min}, 当前 start = {}",
                        r.start
                    )));
                }
                if r.start > r.end {
                    return Err(AppError::Msg(format!(
                        "{label} 起点 {} 不能大于终点 {}",
                        r.start, r.end
                    )));
                }
                if r.step == 0 {
                    return Err(AppError::Msg(format!(
                        "{label}.step 必须 > 0, 当前为 {}",
                        r.step
                    )));
                }
            }
        }
        Ok(())
    }

    /// 消耗自身归一化为范围(单值 → `start == end`)。
    #[must_use]
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

    /// 借用自身归一化为范围(单值 → `start == end`)。
    #[must_use]
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

    /// 展开为具体取值列表。
    ///
    /// 上限 [`MAX_PERIOD_RANGE`] 档防爆(与 f64 展开的 [`MAX_F64_RANGE`] 同思路):
    /// 范围型参数的规模**无上限**(`start = 1, end = 10^9` ⇒ 10 亿档),
    /// 不封顶会把内存吃光。触顶时**告警**而非静默截断。
    #[must_use]
    pub fn values(&self) -> Vec<usize> {
        match self {
            PeriodParam::Single(v) => vec![*v],
            PeriodParam::Range(r) => {
                let mut out = Vec::new();
                // `step == 0` 会让下面的循环**不前进**(死循环)。`validate` 会
                // 单独拒绝它, 但本函数是 `pub` 的, 单独调用时也必须安全。
                if r.step == 0 {
                    return out;
                }
                let mut v = r.start;
                while v <= r.end && out.len() < MAX_PERIOD_RANGE {
                    out.push(v);
                    // `checked_add`: `v + step` 可能越过 `usize::MAX`。
                    match v.checked_add(r.step) {
                        Some(n) => v = n,
                        None => break,
                    }
                }
                if out.len() >= MAX_PERIOD_RANGE && v <= r.end {
                    tracing::warn!(
                        "period 范围展开触顶 {MAX_PERIOD_RANGE} 档(实际想展开到 {}), 已截断",
                        r.end
                    );
                }
                out
            }
        }
    }

    /// 校验展开结果落在 `[lo, hi]` 且非空。
    ///
    /// 与 [`F64Param::validate_range`] 同形, 但边界是**整数**。
    ///
    /// # Errors
    /// 展开为空或存在越界值时返回 [`AppError`]。
    pub fn validate_range(&self, name: &str, lo: usize, hi: usize) -> crate::error::Result<()> {
        let vs = self.values();
        if vs.is_empty() {
            return Err(AppError::Msg(format!("{name} 展开为空")));
        }
        if vs.iter().any(|v| *v < lo || *v > hi) {
            return Err(AppError::Msg(format!("{name} 越界(允许 [{lo}, {hi}])")));
        }
        Ok(())
    }
}

/// 整型参数展开的**档数上限**(防爆)。
///
/// 与 [`crate::strategies::MAX_F64_RANGE`] 同思路: 范围型参数的规模无上限,
/// 不封顶会把内存吃光。
pub const MAX_PERIOD_RANGE: usize = 100_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// 范围参数 `{ start, end, step }`。
pub struct RangeParam<T: PartialOrd + Copy> {
    /// 起点(含)。
    pub start: T,
    /// 终点(含)。
    pub end: T,
    /// 步长(恒 > 0, 由 `validate` 保证)。
    pub step: T,
}

/// f64 参数: 单值或范围(与 PeriodParam 对应, 后者只支持 usize)。
/// TOML 里 `k = 0.2` 与 `k = { start = 0.1, end = 0.6, step = 0.1 }` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 浮点参数: 单值或范围(与 [`PeriodParam`] 对应)。
pub enum F64Param {
    /// 单值形态。
    Single(f64),
    /// 范围形态 `{ start, end, step }`。
    Range(RangeParam<f64>),
}

impl F64Param {
    /// 展开为具体取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        match self {
            F64Param::Single(v) => vec![*v],
            F64Param::Range(r) => crate::strategies::expand_f64_range(r.start, r.end, r.step),
        }
    }

    /// 校验展开结果落在 `[lo, hi]` 且非空。
    ///
    /// # Errors
    /// 展开为空或存在越界/非有限值时返回 [`AppError`]。
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

/// 读取并解析 TOML 配置文件。
///
/// # Errors
/// 文件读取失败, 或 TOML 结构非法(含未知字段 / 类型不匹配)时返回
/// [`AppError`]。
///
/// 收 [`Path`](std::path::Path) 而非 `&str`: 配置路径来自 CLI, 非 UTF-8 路径在 `&str` 边界上
/// 会被迫 `to_str().expect(..)` —— 那是把用户输入的问题变成一次 panic。
pub fn load_config(path: &std::path::Path) -> crate::error::Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
    fn for_pair_produces_single_value_config_for_kline_reader() {
        let cfg: Config = toml::from_str(&make_scan_toml(
            "[\"BTCUSDT\", \"ETHUSDT\"]",
            "[\"15m\", \"1h\"]",
        ))
        .unwrap();
        let pair = cfg.data.for_pair("ETHUSDT", "1h").unwrap();
        assert_eq!(pair.symbol().as_str(), "ETHUSDT");
        assert_eq!(pair.timeframe().as_str(), "1h");
        assert_eq!(pair.dir(), cfg.data.dir);
        assert_eq!(pair.source_type(), cfg.data.source_type);
        assert_eq!(pair.market(), kline_reader::Market::Um);
    }

    #[test]
    fn for_pair_honours_market_override() {
        let toml = "[data]\ntype = \"binance\"\nmarket = \"spot\"\ndir = \"/tmp\"\nsymbol = \"BTCUSDT\"\ntimeframe = \"1d\"\n\n[backtest]\ninitial_balance = 10000.0\n\n[[strategy]]\ntype = \"sma\"\nperiod = 20\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let pair = cfg.data.for_pair("BTCUSDT", "1d").unwrap();
        assert_eq!(pair.market(), kline_reader::Market::Spot);
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
}
