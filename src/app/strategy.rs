//! 策略回测：TOML 入参 → vta 引擎 → 图表就绪 DTO。
//!
//! 前端策略栏填写 vta 策略 TOML 片段（例如 `type = "bbrk"\nsigma = 2.0`），
//! 这里反序列化为 [`vta::config::StrategyConfig`]，经
//! [`vta::strategies::build::build_strategies`] 工厂生成信号，再调
//! [`vta::engine::simulate_detailed`] 回放（`delay = 1` 次根开盘成交口径）。
//! 同一份 TOML 可直接粘进 vta 离线 config 大批量复跑，线上线下同口径。

use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;
use vector_ta::indicators::moving_averages as mav;
use vta::config::{F64Param, MaGridCfg, PeriodParam, StrategyConfig};

use super::data_loader::{DataError, KlineColumns, MarketType};
use super::routes::AppState;

/// 当前支持的策略，同时也是 `/api/strategy` 的放行白名单。
///
/// 首批放行单周期 MA 全家 + BBRK，与前端预设下拉对齐，逐步放开多参数策略。
/// **白名单与下拉列表共用同一份常量**：两份拷贝必然漂移，而漂移的后果是
/// "前端能选、后端拒绝"或反之。
pub const STRATEGIES: &[&str] = &[
    "bbrk",
    "sma",
    "sma_cd",
    "ema",
    "hma",
    "zlma",
    "alma",
    "dema",
    "fwma",
    "jma",
    "kama",
    "lin_reg",
    "pwma",
    "sin_wma",
    "swma",
    "tema",
    "tilson",
    "trima",
    "wilders",
    "wma",
    "smma",
    "ehma",
    "frama",
    "dma",
    "vwma",
    "vidya",
    "mc_ginley",
];

/// 参数网格上限：`build` 之前做算术预检（`SmaStrategy::new` / `BBRKStrategy::new`
/// 会 eager 展开整个网格，`step = 0` 直接 panic，大范围直接 OOM）。
const MAX_GRID: usize = 1024;

/// 策略描述（给前端下拉用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyStub {
    /// 策略标识（与 TOML 的 `type` 一致）。
    pub name: &'static str,
}

/// 列出当前支持的策略。
///
/// 列表项就是 TOML 的 `type` 取值：前端把下拉值原样拼进 `type = "..."`，
/// 两者必须逐字相同（`StrategyConfig` 的 serde tag 是 `snake_case`，
/// 例如 Rust 变体 `Zlma` 对应的 tag 是 `zlma`，不是 `zlema`）。
#[must_use]
pub fn list_strategies() -> Vec<StrategyStub> {
    STRATEGIES
        .iter()
        .copied()
        .map(|name| StrategyStub { name })
        .collect()
}

/// 策略回测错误。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StrategyError {
    /// 400：参数问题（TOML 非法 / 未放行 / 范围爆炸 / 数据为空）。
    #[error("{0}")]
    User(String),
    /// 404：品种/周期无数据（与 `/api/klines` 一致）。
    #[error("{0}")]
    NotFound(String),
    /// 500：引擎内部失败（消息面向日志，响应体统一脱敏为 `internal error`）。
    #[error("{0}")]
    Internal(String),
}

/// 持仓方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeSide {
    /// 多头。
    Long,
    /// 空头。
    Short,
    /// 无持仓（引擎保证成交记录恒为多/空，此变体只为类型完备）。
    Flat,
}

impl From<vta::engine::Side> for TradeSide {
    fn from(side: vta::engine::Side) -> Self {
        match side {
            vta::engine::Side::Long => TradeSide::Long,
            vta::engine::Side::Short => TradeSide::Short,
            vta::engine::Side::Flat => TradeSide::Flat,
        }
    }
}

/// 图表标记类型（前端据此定形状/位置/颜色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    /// 开仓。
    Entry,
    /// 平仓。
    Exit,
}

/// 图表标记（成交点）。
#[derive(Debug, Clone, Serialize)]
pub struct MarkerDto {
    /// 毫秒时间戳，恒对齐某根 bar（delay fill 恒为真实 bar）。
    pub time: i64,
    /// 持仓方向。
    pub side: TradeSide,
    /// 成交价。
    pub price: f64,
    /// 标记类型。
    pub kind: MarkerKind,
}

/// 一笔已平仓交易。
#[derive(Debug, Clone, Serialize)]
pub struct TradeDto {
    /// 开仓 bar 时间戳（毫秒）。
    pub entry_time: i64,
    /// 平仓 bar 时间戳（毫秒）。
    pub exit_time: i64,
    /// 持仓方向。
    pub side: TradeSide,
    /// 开仓成交价。
    pub entry_price: f64,
    /// 平仓成交价。
    pub exit_price: f64,
    /// 复利腿持仓数量。
    pub qty: f64,
    /// 复利腿盈亏。
    pub pnl: f64,
    /// 相对开仓价的收益率。
    pub pnl_pct: f64,
    /// fixed 腿盈亏。
    pub fixed_pnl: f64,
    /// 持仓 bar 数。
    pub bars_held: i64,
}

/// 资金曲线上的一个点。
#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    /// 毫秒时间戳。
    pub time: i64,
    /// 权益值。
    pub value: f64,
}

/// 策略指标线（后端用 signal 同源函数算出，前端只做时间窗过滤+绘制）。
#[derive(Debug, Clone, Serialize)]
pub struct OverlayLineDto {
    /// 线标签（如 `SMA(20)` / `BBU(20,2)`）。
    pub label: String,
    /// 采样点。
    pub points: Vec<EquityPoint>,
}

/// 回测汇总指标。
#[derive(Debug, Clone, Serialize)]
pub struct SummaryDto {
    /// 已平仓交易笔数。
    pub num_trades: u32,
    /// 盈利笔占比（百分数）。
    pub win_rate: f64,
    /// 总收益率（百分数）。
    pub total_return_pct: f64,
    /// 年化夏普。
    pub sharpe_ratio: f64,
    /// 年化索提诺。
    pub sortino_ratio: f64,
    /// 最大回撤（百分数）。
    pub max_drawdown_pct: f64,
    /// 盈利因子。
    pub profit_factor: f64,
    /// 逐笔期望值。
    pub expectancy: f64,
    /// 末根权益。
    pub final_equity: f64,
    /// 年化换手。
    pub turnover_per_year: f64,
    /// fixed 腿总收益率（百分数）。
    pub fixed_total_return: f64,
    /// fixed 腿年化夏普。
    pub fixed_sharpe_ratio: f64,
    /// fixed 腿最大回撤（百分数）。
    pub fixed_max_drawdown_pct: f64,
    /// fixed 腿期望值（基点）。
    pub fixed_expectancy_bps: f64,
    /// fixed 腿盈利因子。
    pub fixed_profit_factor: f64,
}

/// 回测结果（图表就绪 DTO）。
#[derive(Debug, Clone, Serialize)]
pub struct StrategyResult {
    /// 策略标识。
    pub strategy: String,
    /// 回测 bar 数。
    pub count: usize,
    /// 成交标记。
    pub markers: Vec<MarkerDto>,
    /// 已平仓交易。
    pub trades: Vec<TradeDto>,
    /// 资金曲线。
    pub equity: Vec<EquityPoint>,
    /// 指标线。
    pub overlays: Vec<OverlayLineDto>,
    /// 汇总指标。
    pub summary: SummaryDto,
}

/// 回测入参。
#[derive(Debug, Clone)]
pub struct BacktestParams<'a> {
    /// 市场类型。
    pub market_type: MarketType,
    /// 交易对。
    pub pair: &'a str,
    /// 周期。
    pub timeframe: &'a str,
    /// 策略 TOML 片段。
    pub strategy_toml: &'a str,
    /// 初始资金（必须 > 0 且有限）。
    pub initial_balance: f64,
    /// 单边手续费（基点，1 bps = 0.01%）。
    pub fee_bps: f64,
    /// 单边滑点（基点）。
    pub slippage_bps: f64,
    /// 时间窗口上界（不含，毫秒时间戳）。
    pub before: Option<i64>,
    /// 时间窗口下界（不含，毫秒时间戳）。
    pub after: Option<i64>,
}

/// NaN/Inf 清零：`serde_json` 不接受非有限浮点，直接 500。
fn fin(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// vta 的 `equity_curve` 时间戳是 `f64`，回转 `i64`。
///
/// 毫秒时间戳 < 2^53（`f64` 能精确表示的整数上界），故正常路径无损；
/// 超出范围或非有限时退回 0，避免 `as` 转换产生未定义截断。
fn ts_to_i64(ts: f64) -> i64 {
    const EXACT_INT_LIMIT: f64 = 9_007_199_254_740_992.0; // 2^53
    if !ts.is_finite() || ts.abs() >= EXACT_INT_LIMIT {
        return 0;
    }
    ts as i64
}

/// 策略标识（与 TOML 的 `type` 对齐）。
///
/// 未放行的变体统一映射成 `other`，随后被白名单拦下（400）。
fn strategy_id(cfg: &StrategyConfig) -> &'static str {
    match cfg {
        StrategyConfig::Bbrk(_) => "bbrk",
        StrategyConfig::Sma(_) => "sma",
        StrategyConfig::SmaCd(_) => "sma_cd",
        StrategyConfig::Ema(_) => "ema",
        StrategyConfig::Hma(_) => "hma",
        StrategyConfig::Zlma(_) => "zlma",
        StrategyConfig::Alma(_) => "alma",
        StrategyConfig::Dema(_) => "dema",
        StrategyConfig::Fwma(_) => "fwma",
        StrategyConfig::Jma(_) => "jma",
        StrategyConfig::Kama(_) => "kama",
        StrategyConfig::LinReg(_) => "lin_reg",
        StrategyConfig::Pwma(_) => "pwma",
        StrategyConfig::SinWma(_) => "sin_wma",
        StrategyConfig::Swma(_) => "swma",
        StrategyConfig::Tema(_) => "tema",
        StrategyConfig::Tilson(_) => "tilson",
        StrategyConfig::Trima(_) => "trima",
        StrategyConfig::Wilders(_) => "wilders",
        StrategyConfig::Wma(_) => "wma",
        StrategyConfig::Smma(_) => "smma",
        StrategyConfig::Ehma(_) => "ehma",
        StrategyConfig::Frama(_) => "frama",
        StrategyConfig::Dma(_) => "dma",
        StrategyConfig::Vwma(_) => "vwma",
        StrategyConfig::Vidya(_) => "vidya",
        StrategyConfig::McGinley(_) => "mc_ginley",
        _ => "other",
    }
}

/// 取单周期 MA 系的配置（其余变体返回 `None`）。
///
/// MA 系的 TOML 结构完全同构（`MaGridCfg { period }`），用一次 or-pattern
/// 把 25 个变体收拢，避免在网格预检 / 语义校验 / overlay 三处各抄一遍。
fn ma_grid(cfg: &StrategyConfig) -> Option<&MaGridCfg> {
    match cfg {
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
        | StrategyConfig::Smma(c)
        | StrategyConfig::Ehma(c)
        | StrategyConfig::Frama(c)
        | StrategyConfig::Dma(c)
        | StrategyConfig::Vwma(c)
        | StrategyConfig::Vidya(c)
        | StrategyConfig::McGinley(c) => Some(c),
        _ => None,
    }
}

/// 整型网格长度。
///
/// `step == 0`（`checked_div` 返回 `None`）、`end < start`、`start < 1`
/// 都是非法输入；`(end - start) / step + 1` 用 `checked_add` 防溢出
/// （`end = usize::MAX, step = 1` 时会溢出）。
fn usize_grid_len(start: usize, end: usize, step: usize) -> Option<usize> {
    if start < 1 || end < start {
        return None;
    }
    (end - start).checked_div(step)?.checked_add(1)
}

/// 浮点网格长度。
fn f64_grid_len(start: f64, end: f64, step: f64) -> Option<usize> {
    if !step.is_finite() || step <= 0.0 || !start.is_finite() || !end.is_finite() || start > end {
        return None;
    }
    let steps = ((end - start) / step).floor();
    if steps < 0.0 {
        return None;
    }
    // f64 → usize 是饱和转换（Rust 1.45+）：超出 `usize::MAX` 时饱和为 MAX，
    // 随后的 `checked_add` 返回 `None` ⇒ 判为"网格过大"而拒绝。
    (steps as usize).checked_add(1)
}

/// 建实例前的算术预检：返回 `None` 表示非法/爆炸，调用方转 400。
///
/// 只做算术、**不展开网格**：vta 侧的 `SmaStrategy::new` / `BBRKStrategy::new`
/// 会 eager 展开整个参数网格（`step = 0` 直接 panic，大范围直接 OOM）。
fn grid_len(cfg: &StrategyConfig) -> Option<usize> {
    if let Some(c) = ma_grid(cfg) {
        match &c.period {
            PeriodParam::Single(_) => Some(1),
            PeriodParam::Range(r) => usize_grid_len(r.start, r.end, r.step),
        }
    } else if let StrategyConfig::Bbrk(c) = cfg {
        match &c.sigma {
            F64Param::Single(_) => Some(1),
            F64Param::Range(r) => f64_grid_len(r.start, r.end, r.step),
        }
    } else {
        None
    }
}

/// 语义校验：复用 vta 的策略专属约束（`period >= 1` / `sigma ∈ [0.1, 10]`）。
///
/// **必须在网格限幅（[`grid_len`]）之后调用**：`BBRKConfig::validate` 内部会
/// 展开参数网格（`sigma.validate_range` → `values()`），未限幅时大范围输入
/// 会直接 OOM。
fn validate_params(cfg: &StrategyConfig) -> Result<(), String> {
    let res = if let Some(c) = ma_grid(cfg) {
        c.validate()
    } else if let StrategyConfig::Bbrk(c) = cfg {
        c.validate()
    } else {
        return Ok(());
    };
    res.map_err(|e| e.to_string())
}

/// 回测入口。`before/after` 复用 K 线过滤语义（子区间回测，不传即全量）。
///
/// # Errors
/// - 参数非法（余额/费率/网格越界/白名单外）→ [`StrategyError::User`]
/// - 品种/周期无数据 → [`StrategyError::NotFound`]
/// - 引擎内部失败 → [`StrategyError::Internal`]
pub fn run_backtest(
    state: &AppState,
    params: &BacktestParams<'_>,
) -> Result<Arc<StrategyResult>, StrategyError> {
    use StrategyError::{Internal, User};

    let BacktestParams {
        market_type,
        pair,
        timeframe,
        strategy_toml,
        initial_balance,
        fee_bps,
        slippage_bps,
        before,
        after,
    } = *params;

    if !initial_balance.is_finite() || initial_balance <= 0.0 {
        return Err(User("initial_balance must be > 0".into()));
    }
    for (name, v) in [("fee_bps", fee_bps), ("slippage_bps", slippage_bps)] {
        if !v.is_finite() || v < 0.0 {
            return Err(User(format!("{name} must be >= 0")));
        }
    }
    if !vta::strategies::is_known_timeframe(timeframe) {
        return Err(User(format!(
            "unsupported timeframe for annualization: {timeframe}"
        )));
    }

    let cfg: StrategyConfig =
        toml::from_str(strategy_toml).map_err(|e| User(format!("invalid strategy_toml: {e}")))?;
    let id = strategy_id(&cfg);
    if !STRATEGIES.contains(&id) {
        return Err(User(format!(
            "strategy not enabled: {id} (allowlist: {STRATEGIES:?})"
        )));
    }
    // 1) 算术预检：网格规模限幅（vta 侧会 eager 展开网格）。
    match grid_len(&cfg) {
        Some(n) if n <= MAX_GRID => {}
        _ => {
            return Err(User(format!(
                "param grid too large or invalid (max {MAX_GRID})"
            )));
        }
    }
    // 2) 语义校验：复用 vta 的策略专属约束（此时网格已限幅，展开安全）。
    validate_params(&cfg).map_err(|e| User(format!("invalid strategy params: {e}")))?;

    let key = format!(
        "{market_type}/{pair}/{timeframe}/{strategy_toml}/{initial_balance}/{fee_bps}/{slippage_bps}/{before:?}/{after:?}"
    );
    if let Some(hit) = state.strategy_cache.get(&key) {
        return Ok(hit);
    }

    let cols = state
        .data_loader
        .load_columns_filtered(market_type, pair, timeframe, before, after)
        .map_err(|e| match e {
            DataError::NotFound(msg) => StrategyError::NotFound(msg),
            other => StrategyError::Internal(other.to_string()),
        })?;
    if cols.is_empty() {
        return Err(User("no bars in selected range".into()));
    }

    let strategies = vta::strategies::build::build_strategies(std::slice::from_ref(&cfg));
    let strat = strategies
        .into_iter()
        .next()
        .ok_or_else(|| Internal("strategy factory returned empty".into()))?;

    let signals = strat
        .generate_signals_full(
            &cols.open,
            &cols.high,
            &cols.low,
            &cols.close,
            &cols.volume,
            &cols.time,
            0,
        )
        .map_err(|e| Internal(format!("signal error: {e}")))?;
    let exec = vta::engine::ExecConfig::from_bps(1, fee_bps, slippage_bps);
    let sim = vta::engine::simulate_detailed(
        &cols.time,
        &cols.close,
        &cols.open,
        &signals,
        initial_balance,
        vta::strategies::bars_per_year(timeframe),
        &exec,
    );

    let overlays = build_overlays(id, &cfg, &cols);
    let result = Arc::new(to_dto(id, cols.len(), &sim, overlays));
    state.strategy_cache.insert(key, result.clone());
    Ok(result)
}

/// 构造一条指标线（跳过非有限值点）。
fn overlay_line(label: String, values: &[f64], time: &[i64]) -> OverlayLineDto {
    let mut points = Vec::with_capacity(values.len().min(time.len()));
    for (v, t) in values.iter().zip(time.iter()) {
        if v.is_finite() {
            points.push(EquityPoint {
                time: *t,
                value: *v,
            });
        }
    }
    OverlayLineDto { label, points }
}

/// 单均线 overlay：`$input` 由调用方按策略原样构造（与信号同一函数、同一参数）。
macro_rules! ma_overlay {
    ($lines:ident, $time:expr, $label:expr, $calc:path, $input:expr) => {{
        let input = $input;
        if let Ok(out) = $calc(&input) {
            $lines.push(overlay_line($label, &out.values, $time));
        }
    }};
}

/// 策略指标线：与信号同源函数计算（idx=0 档参数），失败则跳过该线。
/// vidya/mcginley 系本地手写实现、无库函数可复用，只返回空（仍有 markers）。
fn build_overlays(id: &str, cfg: &StrategyConfig, cols: &KlineColumns) -> Vec<OverlayLineDto> {
    let close = &cols.close;
    let high = &cols.high;
    let low = &cols.low;
    let volume = &cols.volume;
    let time = &cols.time;

    if id == "bbrk" {
        let sigma = match cfg {
            StrategyConfig::Bbrk(c) => match &c.sigma {
                F64Param::Single(v) => *v,
                F64Param::Range(r) => (r.start * 1e6).round() / 1e6,
            },
            _ => return vec![],
        };
        let input = vector_ta::indicators::bollinger_bands::BollingerBandsInput::from_slice(
            close,
            vector_ta::indicators::bollinger_bands::BollingerBandsParams {
                period: Some(20),
                devup: Some(sigma),
                devdn: Some(sigma),
                matype: Some("sma".to_string()),
                devtype: Some(0),
            },
        );
        let mut lines = Vec::new();
        if let Ok(bb) = vector_ta::indicators::bollinger_bands::bollinger_bands(&input) {
            lines.push(overlay_line(
                format!("BBM(20,{sigma})"),
                &bb.middle_band,
                time,
            ));
            lines.push(overlay_line(
                format!("BBU(20,{sigma})"),
                &bb.upper_band,
                time,
            ));
            lines.push(overlay_line(
                format!("BBL(20,{sigma})"),
                &bb.lower_band,
                time,
            ));
        }
        return lines;
    }

    let period = match ma_grid(cfg) {
        Some(c) => match &c.period {
            PeriodParam::Single(v) => *v,
            PeriodParam::Range(r) => r.start,
        },
        None => return vec![],
    };
    let label = format!("{}({period})", id.to_uppercase());
    let mut lines = Vec::new();
    match id {
        "sma" | "sma_cd" => ma_overlay!(
            lines,
            time,
            label,
            mav::sma::sma,
            mav::sma::SmaInput::from_slice(
                close,
                mav::sma::SmaParams {
                    period: Some(period)
                }
            )
        ),
        "ema" => ma_overlay!(
            lines,
            time,
            label,
            mav::ema::ema,
            mav::ema::EmaInput::from_slice(
                close,
                mav::ema::EmaParams {
                    period: Some(period)
                }
            )
        ),
        "wma" => ma_overlay!(
            lines,
            time,
            label,
            mav::wma::wma,
            mav::wma::WmaInput::from_slice(
                close,
                mav::wma::WmaParams {
                    period: Some(period)
                }
            )
        ),
        "smma" => ma_overlay!(
            lines,
            time,
            label,
            mav::smma::smma,
            mav::smma::SmmaInput::from_slice(
                close,
                mav::smma::SmmaParams {
                    period: Some(period)
                }
            )
        ),
        "ehma" => ma_overlay!(
            lines,
            time,
            label,
            mav::ehma::ehma,
            mav::ehma::EhmaInput::from_slice(
                close,
                mav::ehma::EhmaParams {
                    period: Some(period)
                }
            )
        ),
        "frama" => ma_overlay!(
            lines,
            time,
            label,
            mav::frama::frama,
            mav::frama::FramaInput::from_slices(
                high,
                low,
                close,
                mav::frama::FramaParams {
                    window: Some(period),
                    sc: Some(300),
                    fc: Some(1)
                }
            )
        ),
        "dma" => ma_overlay!(
            lines,
            time,
            label,
            mav::dma::dma,
            mav::dma::DmaInput::from_slice(
                close,
                mav::dma::DmaParams {
                    hull_length: Some(period),
                    ..Default::default()
                }
            )
        ),
        "vwma" => ma_overlay!(
            lines,
            time,
            label,
            mav::vwma::vwma,
            mav::vwma::VwmaInput::from_slice(
                close,
                volume,
                mav::vwma::VwmaParams {
                    period: Some(period)
                }
            )
        ),
        "tilson" => ma_overlay!(
            lines,
            time,
            label,
            mav::tilson::tilson,
            mav::tilson::TilsonInput::from_slice(
                close,
                mav::tilson::TilsonParams {
                    period: Some(period),
                    volume_factor: None
                }
            )
        ),
        "jma" => ma_overlay!(
            lines,
            time,
            label,
            mav::jma::jma,
            mav::jma::JmaInput::from_slice(
                close,
                mav::jma::JmaParams {
                    period: Some(period),
                    phase: None,
                    power: None
                }
            )
        ),
        "kama" => ma_overlay!(
            lines,
            time,
            label,
            mav::kama::kama,
            mav::kama::KamaInput::from_slice(
                close,
                mav::kama::KamaParams {
                    period: Some(period)
                }
            )
        ),
        "lin_reg" => ma_overlay!(
            lines,
            time,
            label,
            mav::linreg::linreg,
            mav::linreg::LinRegInput::from_slice(
                close,
                mav::linreg::LinRegParams {
                    period: Some(period)
                }
            )
        ),
        "pwma" => ma_overlay!(
            lines,
            time,
            label,
            mav::pwma::pwma,
            mav::pwma::PwmaInput::from_slice(
                close,
                mav::pwma::PwmaParams {
                    period: Some(period)
                }
            )
        ),
        "sin_wma" => ma_overlay!(
            lines,
            time,
            label,
            mav::sinwma::sinwma,
            mav::sinwma::SinWmaInput::from_slice(
                close,
                mav::sinwma::SinWmaParams {
                    period: Some(period)
                }
            )
        ),
        "swma" => ma_overlay!(
            lines,
            time,
            label,
            mav::swma::swma,
            mav::swma::SwmaInput::from_slice(
                close,
                mav::swma::SwmaParams {
                    period: Some(period)
                }
            )
        ),
        "tema" => ma_overlay!(
            lines,
            time,
            label,
            mav::tema::tema,
            mav::tema::TemaInput::from_slice(
                close,
                mav::tema::TemaParams {
                    period: Some(period)
                }
            )
        ),
        "trima" => ma_overlay!(
            lines,
            time,
            label,
            mav::trima::trima,
            mav::trima::TrimaInput::from_slice(
                close,
                mav::trima::TrimaParams {
                    period: Some(period)
                }
            )
        ),
        "wilders" => ma_overlay!(
            lines,
            time,
            label,
            mav::wilders::wilders,
            mav::wilders::WildersInput::from_slice(
                close,
                mav::wilders::WildersParams {
                    period: Some(period)
                }
            )
        ),
        "zlma" => ma_overlay!(
            lines,
            time,
            label,
            mav::zlema::zlema,
            mav::zlema::ZlemaInput::from_slice(
                close,
                mav::zlema::ZlemaParams {
                    period: Some(period)
                }
            )
        ),
        "hma" => ma_overlay!(
            lines,
            time,
            label,
            mav::hma::hma,
            mav::hma::HmaInput::from_slice(
                close,
                mav::hma::HmaParams {
                    period: Some(period)
                }
            )
        ),
        "alma" => ma_overlay!(
            lines,
            time,
            label,
            mav::alma::alma,
            mav::alma::AlmaInput::from_slice(
                close,
                mav::alma::AlmaParams {
                    period: Some(period),
                    offset: None,
                    sigma: None
                }
            )
        ),
        "dema" => ma_overlay!(
            lines,
            time,
            label,
            mav::dema::dema,
            mav::dema::DemaInput::from_slice(
                close,
                mav::dema::DemaParams {
                    period: Some(period)
                }
            )
        ),
        "fwma" => ma_overlay!(
            lines,
            time,
            label,
            mav::fwma::fwma,
            mav::fwma::FwmaInput::from_slice(
                close,
                mav::fwma::FwmaParams {
                    period: Some(period)
                }
            )
        ),
        // vidya/mcginley 系策略本地手写实现、无库函数可复用：只返回空（仍有 markers）。
        _ => {}
    }
    lines
}

/// 引擎结果 → DTO。
fn to_dto(
    id: &str,
    count: usize,
    sim: &vta::engine::SimResult,
    overlays: Vec<OverlayLineDto>,
) -> StrategyResult {
    let mut markers = Vec::with_capacity(sim.trades.len() * 2);
    let mut trades = Vec::with_capacity(sim.trades.len());
    for t in &sim.trades {
        let side = TradeSide::from(t.side);
        markers.push(MarkerDto {
            time: t.entry_time,
            side,
            price: fin(t.entry_price),
            kind: MarkerKind::Entry,
        });
        markers.push(MarkerDto {
            time: t.exit_time,
            side,
            price: fin(t.exit_price),
            kind: MarkerKind::Exit,
        });
        trades.push(TradeDto {
            entry_time: t.entry_time,
            exit_time: t.exit_time,
            side,
            entry_price: fin(t.entry_price),
            exit_price: fin(t.exit_price),
            qty: fin(t.qty),
            pnl: fin(t.pnl),
            pnl_pct: fin(t.pnl_pct),
            fixed_pnl: fin(t.fixed_pnl),
            bars_held: t.bars_held,
        });
    }
    markers.sort_by_key(|m| m.time);
    let equity = sim
        .equity_curve
        .iter()
        .map(|(ts, eq)| EquityPoint {
            time: ts_to_i64(*ts),
            value: fin(*eq),
        })
        .collect();
    let s = &sim.summary;
    StrategyResult {
        strategy: id.into(),
        count,
        markers,
        trades,
        equity,
        overlays,
        summary: SummaryDto {
            num_trades: s.num_trades,
            win_rate: fin(s.win_rate),
            total_return_pct: fin(s.total_return_pct),
            sharpe_ratio: fin(s.sharpe_ratio),
            sortino_ratio: fin(s.sortino_ratio),
            max_drawdown_pct: fin(s.max_drawdown_pct),
            profit_factor: fin(s.profit_factor),
            expectancy: fin(s.expectancy),
            final_equity: fin(s.final_equity),
            turnover_per_year: fin(s.turnover_per_year),
            fixed_total_return: fin(s.fixed_total_return),
            fixed_sharpe_ratio: fin(s.fixed_sharpe_ratio),
            fixed_max_drawdown_pct: fin(s.fixed_max_drawdown_pct),
            fixed_expectancy_bps: fin(s.fixed_expectancy_bps),
            fixed_profit_factor: fin(s.fixed_profit_factor),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_allowlisted_strategies() {
        let names: Vec<&str> = list_strategies().iter().map(|s| s.name).collect();
        assert!(names.contains(&"bbrk"));
        assert!(names.contains(&"hma"));
        assert_eq!(names.len(), STRATEGIES.len());
    }

    #[test]
    fn rejects_bad_toml() {
        let bad: Result<StrategyConfig, _> = toml::from_str("type = \"nope\"\nperiod = 20");
        assert!(bad.is_err());
    }

    #[test]
    fn rejects_huge_grid_before_build() {
        let cfg: StrategyConfig =
            toml::from_str("type = \"sma\"\nperiod = { start = 1, end = 99999999, step = 1 }")
                .unwrap();
        assert!(grid_len(&cfg).unwrap() > MAX_GRID);
    }

    #[test]
    fn rejects_zero_step_before_build() {
        let cfg: StrategyConfig =
            toml::from_str("type = \"sma\"\nperiod = { start = 1, end = 10, step = 0 }").unwrap();
        assert!(grid_len(&cfg).is_none());
    }

    #[test]
    fn rejects_zero_start_before_build() {
        let cfg: StrategyConfig =
            toml::from_str("type = \"sma\"\nperiod = { start = 0, end = 10, step = 1 }").unwrap();
        assert!(grid_len(&cfg).is_none());
    }

    #[test]
    fn grid_len_survives_extreme_range() {
        // end = usize::MAX 时网格长度本身就是 usize::MAX（start >= 1 ⇒ 不会溢出），
        // 由 MAX_GRID 判定拒绝；而 step = 0 / 区间反向必须直接判非法。
        assert_eq!(usize_grid_len(1, usize::MAX, 1), Some(usize::MAX));
        assert_eq!(usize_grid_len(1, usize::MAX, usize::MAX), Some(1));
        assert_eq!(usize_grid_len(1, 10, 3), Some(4));
        assert_eq!(usize_grid_len(1, 10, 0), None);
        assert_eq!(usize_grid_len(10, 1, 1), None);
        assert_eq!(usize_grid_len(0, 10, 1), None);
    }

    #[test]
    fn f64_grid_len_survives_extreme_range() {
        assert_eq!(f64_grid_len(1.0, 2.0, 0.5), Some(3));
        assert_eq!(f64_grid_len(1.0, 2.0, 0.0), None);
        assert_eq!(f64_grid_len(1.0, 2.0, -0.5), None);
        assert_eq!(f64_grid_len(2.0, 1.0, 0.5), None);
        // 极大跨度 ⇒ 饱和 ⇒ 判为过大
        assert_eq!(f64_grid_len(0.0, 1e300, 1e-300), None);
    }

    #[test]
    fn validate_params_rejects_out_of_range_sigma() {
        let cfg: StrategyConfig = toml::from_str("type = \"bbrk\"\nsigma = 100.0").unwrap();
        assert!(validate_params(&cfg).is_err());
    }

    #[test]
    fn validate_params_accepts_valid_config() {
        let cfg: StrategyConfig = toml::from_str("type = \"sma\"\nperiod = 20").unwrap();
        assert!(validate_params(&cfg).is_ok());
        let cfg: StrategyConfig = toml::from_str("type = \"bbrk\"\nsigma = 2.0").unwrap();
        assert!(validate_params(&cfg).is_ok());
    }

    #[test]
    fn validate_params_rejects_zero_period() {
        let cfg: StrategyConfig = toml::from_str("type = \"sma\"\nperiod = 0").unwrap();
        assert!(validate_params(&cfg).is_err());
    }

    #[test]
    fn every_allowlisted_strategy_is_reachable() {
        // 白名单里的每个 id 都必须能被 `strategy_id` 映射回来：否则前端能选、
        // 后端却映射成 "other" 而被白名单拒掉（下拉与白名单已经共用一份常量，
        // 这里再钉住"id ↔ 变体"的映射）。
        for name in STRATEGIES {
            let toml_src = if *name == "bbrk" {
                "type = \"bbrk\"\nsigma = 2.0".to_string()
            } else {
                format!("type = \"{name}\"\nperiod = 20")
            };
            let cfg: StrategyConfig = toml::from_str(&toml_src).unwrap();
            assert_eq!(strategy_id(&cfg), *name);
        }
    }

    #[test]
    fn strategy_error_is_std_error() {
        let e = StrategyError::User("bad".into());
        let dyn_err: &dyn std::error::Error = &e;
        assert_eq!(dyn_err.to_string(), "bad");
    }

    #[test]
    fn fin_sanitizes_non_finite() {
        assert_eq!(fin(f64::NAN), 0.0);
        assert_eq!(fin(f64::INFINITY), 0.0);
        assert_eq!(fin(1.5), 1.5);
    }

    #[test]
    fn ts_to_i64_roundtrips_millis() {
        assert_eq!(ts_to_i64(1_700_000_000_000.0), 1_700_000_000_000);
        assert_eq!(ts_to_i64(f64::NAN), 0);
        assert_eq!(ts_to_i64(1e300), 0);
    }

    fn trend_columns() -> KlineColumns {
        let n = 60usize;
        KlineColumns {
            time: (0..n)
                .map(|i| 1_700_000_000_000 + i as i64 * 3_600_000)
                .collect(),
            open: (0..n).map(|i| 100.0 + i as f64 * 0.5).collect(),
            high: (0..n).map(|i| 101.0 + i as f64 * 0.5).collect(),
            low: (0..n).map(|i| 99.0 + i as f64 * 0.5).collect(),
            close: (0..n).map(|i| 100.0 + i as f64 * 0.5).collect(),
            volume: vec![10.0; n],
        }
    }

    #[test]
    fn overlays_come_from_signal_source() {
        let cols = trend_columns();
        let cfg: StrategyConfig = toml::from_str("type = \"bbrk\"\nsigma = 2.0").unwrap();
        let lines = build_overlays("bbrk", &cfg, &cols);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| !l.points.is_empty()));

        let cfg: StrategyConfig = toml::from_str("type = \"hma\"\nperiod = 20").unwrap();
        let lines = build_overlays("hma", &cfg, &cols);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].points.is_empty());

        // vidya 系本地实现、无 overlay 线（仍有 markers，不断言）。
        let cfg: StrategyConfig = toml::from_str("type = \"vidya\"\nperiod = 20").unwrap();
        let lines = build_overlays("vidya", &cfg, &cols);
        assert!(lines.is_empty());
    }
}
