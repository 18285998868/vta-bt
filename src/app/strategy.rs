//! Strategy backtest seam: TOML-in → vta engine → chart-ready DTOs.
//!
//! 前端策略栏填写 vta 策略 TOML 片段（例如 `type = "bbrk"\nsigma = 2.0`），
//! 这里反序列化为 [`vta::config::StrategyConfig`]，经
//! [`vta::strategies::build::build_strategies`] 工厂生成信号，再调
//! [`vta::engine::simulate_detailed`] 回放（`delay = 1` 次根开盘成交口径）。
//! 同一份 TOML 可直接粘进 vta 离线 config 大批量复跑，线上线下同口径。

use std::sync::Arc;

use serde::Serialize;
use vector_ta::indicators::moving_averages as mav;
use vta::config::{F64Param, PeriodParam, StrategyConfig};

use super::data_loader::{DataError, Kline, MarketType};
use super::routes::AppState;

/// 首批放行的策略（单周期 MA 全家 + BBRK，与前端预设下拉对齐，逐步放开多参数策略）。
const ALLOWLIST: &[&str] = &[
    "bbrk",
    "sma",
    "sma_cd",
    "ema",
    "hma",
    "zlema",
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
/// 参数网格上限：`build` 之前做算术预检（`SmaStrategy::new` 会 eager 展开，
/// `step = 0` 直接 panic，大范围直接 OOM）。
const MAX_GRID: usize = 1024;

/// 当前支持的策略（`/api/strategies` 下拉数据源）。
pub const STRATEGIES: &[&str] = &[
    "bbrk",
    "sma",
    "sma_cd",
    "ema",
    "hma",
    "zlema",
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

/// 策略描述（给前端下拉用）。
#[derive(Debug, Clone)]
pub struct StrategyStub {
    pub name: &'static str,
}

pub fn list_strategies() -> Vec<StrategyStub> {
    STRATEGIES
        .iter()
        .copied()
        .map(|name| StrategyStub { name })
        .collect()
}

#[derive(Debug)]
pub enum StrategyError {
    /// 400：参数问题（TOML 非法 / 未放行 / 范围爆炸 / 数据为空）。
    User(String),
    /// 404：品种/周期无数据（与 `/api/klines` 一致）。
    NotFound(String),
    /// 500：引擎内部失败。
    Internal(String),
}

impl std::fmt::Display for StrategyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyError::User(msg) => write!(f, "{msg}"),
            StrategyError::NotFound(msg) => write!(f, "{msg}"),
            StrategyError::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkerDto {
    /// 毫秒时间戳，恒对齐某根 bar（delay fill 恒为真实 bar）。
    pub time: i64,
    /// "long" | "short"。
    pub side: String,
    pub price: f64,
    /// "entry" | "exit"（前端据此定形状/位置/颜色）。
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeDto {
    pub entry_time: i64,
    pub exit_time: i64,
    pub side: String,
    pub entry_price: f64,
    pub exit_price: f64,
    pub qty: f64,
    pub pnl: f64,
    pub pnl_pct: f64,
    pub fixed_pnl: f64,
    pub bars_held: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub time: i64,
    pub value: f64,
}

/// 策略指标线（后端用 signal 同源函数算出，前端只做时间窗过滤+绘制）。
#[derive(Debug, Clone, Serialize)]
pub struct OverlayLineDto {
    pub label: String,
    pub points: Vec<EquityPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryDto {
    pub num_trades: u32,
    pub win_rate: f64,
    pub total_return_pct: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub max_drawdown_pct: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
    pub final_equity: f64,
    pub turnover_per_year: f64,
    pub fixed_total_return: f64,
    pub fixed_sharpe_ratio: f64,
    pub fixed_max_drawdown_pct: f64,
    pub fixed_expectancy_bps: f64,
    pub fixed_profit_factor: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyResult {
    pub strategy: String,
    pub count: usize,
    pub markers: Vec<MarkerDto>,
    pub trades: Vec<TradeDto>,
    pub equity: Vec<EquityPoint>,
    pub overlays: Vec<OverlayLineDto>,
    pub summary: SummaryDto,
}

/// NaN/Inf 清零：`serde_json` 不接受非有限浮点，直接 500。
fn fin(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

fn strategy_id(cfg: &StrategyConfig) -> &'static str {
    match cfg {
        StrategyConfig::Bbrk(_) => "bbrk",
        StrategyConfig::Sma(_) => "sma",
        StrategyConfig::SmaCd(_) => "sma_cd",
        StrategyConfig::Ema(_) => "ema",
        StrategyConfig::Hma(_) => "hma",
        StrategyConfig::Zlma(_) => "zlema",
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

fn usize_grid_len(start: usize, end: usize, step: usize) -> Option<usize> {
    if step == 0 || end < start {
        return None;
    }
    Some((end - start) / step + 1)
}

fn f64_grid_len(start: f64, end: f64, step: f64) -> Option<usize> {
    if !step.is_finite() || step <= 0.0 || !start.is_finite() || !end.is_finite() || start > end {
        return None;
    }
    Some(((end - start) / step).floor() as usize + 1)
}

/// 建实例前的算术预检：返回 `None` 表示非法/爆炸，调用方转 400。
fn grid_len(cfg: &StrategyConfig) -> Option<usize> {
    match cfg {
        StrategyConfig::Bbrk(c) => match &c.sigma {
            F64Param::Single(_) => Some(1),
            F64Param::Range(r) => f64_grid_len(r.start, r.end, r.step),
        },
        // 单周期 MA 全家同构 MaGridCfg。
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
        | StrategyConfig::McGinley(c) => match &c.period {
            PeriodParam::Single(_) => Some(1),
            PeriodParam::Range(r) => usize_grid_len(r.start, r.end, r.step),
        },
        _ => None,
    }
}

/// 回测入口。`before/after` 复用 K 线过滤语义（子区间回测，不传即全量）。
#[allow(clippy::too_many_arguments)]
pub fn run_backtest(
    state: &AppState,
    market_type: MarketType,
    pair: &str,
    timeframe: &str,
    strategy_toml: &str,
    initial_balance: f64,
    fee_bps: f64,
    slippage_bps: f64,
    before: Option<i64>,
    after: Option<i64>,
) -> Result<Arc<StrategyResult>, StrategyError> {
    use StrategyError::{Internal, User};

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
    if !ALLOWLIST.contains(&id) {
        return Err(User(format!(
            "strategy not enabled: {id} (allowlist: {ALLOWLIST:?})"
        )));
    }
    match grid_len(&cfg) {
        Some(n) if n <= MAX_GRID => {}
        _ => {
            return Err(User(format!(
                "param grid too large or invalid (max {MAX_GRID})"
            )));
        }
    }

    let key = format!(
        "{market_type}/{pair}/{timeframe}/{strategy_toml}/{initial_balance}/{fee_bps}/{slippage_bps}/{before:?}/{after:?}"
    );
    if let Some(hit) = state.strategy_cache.get(&key) {
        return Ok(hit);
    }

    let klines: Vec<Kline> = state
        .data_loader
        .load_klines_filtered(market_type, pair, timeframe, before, after, None)
        .map_err(|e| match e {
            DataError::NotFound(msg) => StrategyError::NotFound(msg),
            other => StrategyError::Internal(other.to_string()),
        })?;
    if klines.is_empty() {
        return Err(User("no bars in selected range".into()));
    }

    let strategies = vta::strategies::build::build_strategies(std::slice::from_ref(&cfg));
    let strat = strategies
        .into_iter()
        .next()
        .ok_or_else(|| Internal("strategy factory returned empty".into()))?;

    let n = klines.len();
    let mut time = Vec::with_capacity(n);
    let mut open = Vec::with_capacity(n);
    let mut high = Vec::with_capacity(n);
    let mut low = Vec::with_capacity(n);
    let mut close = Vec::with_capacity(n);
    let mut volume = Vec::with_capacity(n);
    for k in &klines {
        time.push(k.time);
        open.push(k.open);
        high.push(k.high);
        low.push(k.low);
        close.push(k.close);
        volume.push(k.volume);
    }

    let signals = strat
        .generate_signals_full(&open, &high, &low, &close, &volume, &time, 0)
        .map_err(|e| Internal(format!("signal error: {e}")))?;
    let exec = vta::engine::ExecConfig::from_bps(1, fee_bps, slippage_bps);
    let sim = vta::engine::simulate_detailed(
        &time,
        &close,
        &open,
        &signals,
        initial_balance,
        vta::strategies::bars_per_year(timeframe),
        &exec,
    );

    let overlays = build_overlays(id, &cfg, &close, &high, &low, &volume, &time);
    let result = Arc::new(to_dto(id, n, &sim, overlays));
    state.strategy_cache.insert(key, result.clone());
    Ok(result)
}

fn overlay_line(label: String, values: &[f64], time: &[i64]) -> OverlayLineDto {
    let mut points = Vec::new();
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
#[allow(clippy::too_many_arguments)]
fn build_overlays(
    id: &str,
    cfg: &StrategyConfig,
    close: &[f64],
    high: &[f64],
    low: &[f64],
    volume: &[f64],
    time: &[i64],
) -> Vec<OverlayLineDto> {
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

    let period = match cfg {
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
        | StrategyConfig::McGinley(c) => match &c.period {
            PeriodParam::Single(v) => *v,
            PeriodParam::Range(r) => r.start,
        },
        _ => return vec![],
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
        "zlema" => ma_overlay!(
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

fn to_dto(
    id: &str,
    count: usize,
    sim: &vta::engine::SimResult,
    overlays: Vec<OverlayLineDto>,
) -> StrategyResult {
    let mut markers = Vec::with_capacity(sim.trades.len() * 2);
    let mut trades = Vec::with_capacity(sim.trades.len());
    for t in &sim.trades {
        let side = if t.side == 1 { "long" } else { "short" };
        markers.push(MarkerDto {
            time: t.entry_time,
            side: side.into(),
            price: fin(t.entry_price),
            kind: "entry".into(),
        });
        markers.push(MarkerDto {
            time: t.exit_time,
            side: side.into(),
            price: fin(t.exit_price),
            kind: "exit".into(),
        });
        trades.push(TradeDto {
            entry_time: t.entry_time,
            exit_time: t.exit_time,
            side: side.into(),
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
            time: *ts as i64,
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
        assert_eq!(names.len(), ALLOWLIST.len());
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
    fn fin_sanitizes_non_finite() {
        assert_eq!(fin(f64::NAN), 0.0);
        assert_eq!(fin(f64::INFINITY), 0.0);
        assert_eq!(fin(1.5), 1.5);
    }

    fn trend_closes() -> (Vec<f64>, Vec<i64>) {
        let close: Vec<f64> = (0..60).map(|i| 100.0 + i as f64 * 0.5).collect();
        let time: Vec<i64> = (0..60).map(|i| 1_700_000_000_000 + i * 3_600_000).collect();
        (close, time)
    }

    #[test]
    fn overlays_come_from_signal_source() {
        let (close, time) = trend_closes();
        let zeros = vec![0.0; 60];
        let cfg: StrategyConfig = toml::from_str("type = \"bbrk\"\nsigma = 2.0").unwrap();
        let lines = build_overlays("bbrk", &cfg, &close, &close, &close, &zeros, &time);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| !l.points.is_empty()));

        let cfg: StrategyConfig = toml::from_str("type = \"hma\"\nperiod = 20").unwrap();
        let lines = build_overlays("hma", &cfg, &close, &close, &close, &zeros, &time);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].points.is_empty());

        // vidya 系本地实现、无 overlay 线（仍有 markers，不断言）。
        let cfg: StrategyConfig = toml::from_str("type = \"vidya\"\nperiod = 20").unwrap();
        let lines = build_overlays("vidya", &cfg, &close, &close, &close, &zeros, &time);
        assert!(lines.is_empty());
    }
}
