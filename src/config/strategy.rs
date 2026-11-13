//! 策略配置: 单条策略、参数网格与范围参数。

use serde::Deserialize;

use crate::engine::StopConfig;
use crate::error::AppError;
use crate::strategies::aberration::AberrationConfig;
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
use crate::strategies::clenow::ClenowConfig;
use crate::strategies::dualthrust::DualThrustConfig;
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
    /// 该策略的仓位模式; 缺省全仓复利。
    #[serde(default)]
    pub sizing: crate::engine::SizingConfig,
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
    /// `period` 越界时返回 `AppError`。
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
    /// **Ssf** 策略: 配置 `SsfConfig`。
    Ssf(SsfConfig),
    /// **SsfCd** 策略: 配置 `SsfCdConfig`。
    SsfCd(SsfCdConfig),
    /// **SsfSt** 策略: 配置 `SsfStConfig`。
    SsfSt(SsfStConfig),
    /// **SsfStCd** 策略: 配置 `SsfStCdConfig`。
    SsfStCd(SsfStCdConfig),
    /// **SwingFractal** 策略: 配置 `SwingFractalConfig`。 TOML 中写作 `type = "swing_fractal"`。
    SwingFractal(SwingFractalConfig),
    /// **Turtle** 策略: 配置 `TurtleConfig`。
    Turtle(TurtleConfig),
    /// **Clenow** 策略: 配置 `ClenowConfig`。 TOML 中写作 `type = "clenow"`。
    Clenow(ClenowConfig),
    /// **DualThrust** 策略: 配置 `DualThrustConfig`。
    DualThrust(DualThrustConfig),
    /// **RBreaker** 策略: 配置 `RBreakerConfig`。
    RBreaker(RBreakerConfig),
    /// **Fiali** 策略: 配置 `FialiConfig`。
    Fiali(FialiConfig),
    /// **Aberration** 策略: 配置 `AberrationConfig`。
    Aberration(AberrationConfig),
    /// **Sgma** 策略: 配置 `SgmaConfig`。
    Sgma(SgmaConfig),
    #[serde(rename = "atr_band")]
    /// **ATRBand** 策略: 配置 `ATRBandConfig`。 TOML 中写作 `type = "atr_band"`。
    ATRBand(ATRBandConfig),
    /// **Keltner** 策略: 配置 `KeltnerConfig`。
    Keltner(KeltnerConfig),
    #[serde(rename = "bbmr")]
    /// **BBMR** 策略: 配置 `BBMRConfig`。 TOML 中写作 `type = "bbmr"`。
    BBMR(BBMRConfig),
    /// **Bbrk** 策略: 配置 `BBRKConfig`。
    Bbrk(BBRKConfig),
    /// **BbrkAdx** 策略: 配置 `BbrkAdxConfig`。
    BbrkAdx(BbrkAdxConfig),
    /// **BbrkTrail** 策略: 配置 `BbrkTrailConfig`。
    BbrkTrail(BbrkTrailConfig),
    /// **BbrkDon** 策略: 配置 `BbrkDonConfig`。
    BbrkDon(BbrkDonConfig),
    /// **BbrkEma** 策略: 配置 `BbrkEmaConfig`。
    BbrkEma(BbrkEmaConfig),
    /// **BbrkPctb** 策略: 配置 `BbrkPctbConfig`。
    BbrkPctb(BbrkPctbConfig),
    /// **BbrkAtr** 策略: 配置 `BbrkAtrConfig`。
    BbrkAtr(BbrkAtrConfig),
    /// **BbrkStr** 策略: 配置 `BbrkStrConfig`。
    BbrkStr(BbrkStrConfig),
    /// **BbrkSma** 策略: 配置 `BbrkSmaConfig`。
    BbrkSma(BbrkSmaConfig),
    /// **BbrkHma** 策略: 配置 `BbrkHmaConfig`。
    BbrkHma(BbrkHmaConfig),
    /// **BbrkMax** 策略: 配置 `BbrkMaxConfig`。
    BbrkMax(BbrkMaxConfig),
    /// **BbrkBb** 策略: 配置 `BbrkBbConfig`。
    BbrkBb(BbrkBbConfig),
    /// **BbrkVolEma** 策略: 配置 `BbrkVolEmaConfig`。
    BbrkVolEma(BbrkVolEmaConfig),
    /// **BbrkAtrEma** 策略: 配置 `BbrkAtrEmaConfig`。
    BbrkAtrEma(BbrkAtrEmaConfig),
    /// **BbrkSmaEma** 策略: 配置 `BbrkSmaEmaConfig`。
    BbrkSmaEma(BbrkSmaEmaConfig),
    /// **BbrkHmaEma** 策略: 配置 `BbrkHmaEmaConfig`。
    BbrkHmaEma(BbrkHmaEmaConfig),
    /// **BbrkHmaV1** 策略: 配置 `BbrkHmaV1Config`。
    BbrkHmaV1(BbrkHmaV1Config),
    /// **BbrkHmaV2** 策略: 配置 `BbrkHmaV2Config`。
    BbrkHmaV2(BbrkHmaV2Config),
    /// **BbrkHmaV3** 策略: 配置 `BbrkHmaV3Config`。
    BbrkHmaV3(BbrkHmaV3Config),
    /// **BbrkHmaV4** 策略: 配置 `BbrkHmaV4Config`。
    BbrkHmaV4(BbrkHmaV4Config),
    /// **BbrkHmaV5** 策略: 配置 `BbrkHmaV5Config`。
    BbrkHmaV5(BbrkHmaV5Config),
    /// **BbrkZlemaEma** 策略: 配置 `BbrkZlemaEmaConfig`。
    BbrkZlemaEma(BbrkZlemaEmaConfig),
    /// **BbrkRex** 策略: 配置 `BbrkRexConfig`。
    BbrkRex(BbrkRexConfig),
    /// **BbrkVol** 策略: 配置 `BbrkVolConfig`。
    BbrkVol(BbrkVolConfig),
    /// **BbrkTtm** 策略: 配置 `BbrkTtmConfig`。
    BbrkTtm(BbrkTtmConfig),
    /// **BbrkTtmFire** 策略: 配置 `BbrkTtmFireConfig`。
    BbrkTtmFire(BbrkTtmFireConfig),
    /// **BbrkCross** 策略: 配置 `BbrkCrossConfig`。
    BbrkCross(BbrkCrossConfig),
    /// **BbrkKdj** 策略: 配置 `BbrkKdjConfig`。
    BbrkKdj(BbrkKdjConfig),
    /// **BbrkM2** 策略: 配置 `BbrkM2Config`。
    BbrkM2(BbrkM2Config),
    /// **BbrkM3** 策略: 配置 `BbrkM3Config`。
    BbrkM3(BbrkM3Config),
    /// **BbrkOrth** 策略: 配置 `BbrkOrthConfig`。
    BbrkOrth(BbrkOrthConfig),
    /// **VwapTrend** 策略: 配置 `VwapTrendConfig`。
    VwapTrend(VwapTrendConfig),
    #[serde(rename = "rsi2")]
    /// **RSI2** 策略: 配置 `RSI2Config`。 TOML 中写作 `type = "rsi2"`。
    RSI2(RSI2Config),
    /// **Vwap** 策略: 配置 `VwapConfig`。
    Vwap(VwapConfig),
    /// **MabbLong** 策略: 配置 `MabbLongConfig`。
    MabbLong(MabbLongConfig),
    /// **MabbShort** 策略: 配置 `MabbShortConfig`。
    MabbShort(MabbShortConfig),
    /// **MaAtr** 策略: 配置 `MaAtrConfig`。
    MaAtr(MaAtrConfig),
    /// **MaSlopeAtrGate** 策略: 配置 `MaSlopeAtrGateConfig`。
    MaSlopeAtrGate(MaSlopeAtrGateConfig),
    /// **Macd** 策略: 配置 `MacdConfig`。
    Macd(MacdConfig),
    /// **EmaCross** 策略: 配置 `EmaCrossConfig`。
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
    /// 任一约束不满足时返回 `AppError`。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.validate_named("period", 1)
    }

    /// 语义校验, **参数名可定制**且允许非 1 的下界。
    ///
    /// 为什么需要它: 策略配置里除 `period` 外还有"步长型"整型参数
    /// (`sgma.poly_order` 多项式阶数), 它们的合法下界与字段名都和 `period`
    /// 不同。用 `validate()` 一刀切会误拒合法值
    /// (例如 `poly_order = 0`), 而**不校验**又会漏掉 `step = 0` ——
    /// 后者会在展开网格时触发 `Range::step_by(0)` 的 panic。
    ///
    /// # Errors
    /// 任一约束不满足时返回 `AppError`, 消息里带 `label` 便于定位。
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
    /// 上限 [`MAX_PERIOD_RANGE`] 档防爆(与 f64 展开的 `MAX_F64_RANGE` 同思路):
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
    /// 展开为空或存在越界值时返回 `AppError`。
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
/// 与 `crate::strategies::MAX_F64_RANGE` 同思路: 范围型参数的规模无上限,
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
    /// 展开为空或存在越界/非有限值时返回 `AppError`。
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
