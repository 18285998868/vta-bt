//! 分析 API 的公开数据模型。
//!
//! 所有结构都直接序列化为 `analysis.json`; 字段命名保持稳定, 变更时提升
//! [`SCHEMA_VERSION`]。Markdown/CSV 只是这些结构的视图, 不额外计算指标。

use serde::Serialize;

use vta_core::RunResult;

use crate::bootstrap::BootstrapResult;
use crate::trades::{CountItem, TradeStats};

/// `analysis.json` 的 schema 版本。
pub const SCHEMA_VERSION: u32 = 2;

/// 分析参数。
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeOptions {
    /// 显著性水平, 用于 MinTRL 与 bootstrap 置信区间。默认 0.05。
    pub alpha: f64,
    /// 平稳 bootstrap 重采样次数; 0 表示跳过。默认 2000。
    pub bootstrap: usize,
    /// 平稳 bootstrap 块长(bar 数); `None` 表示 Politis-White 自动选择。
    pub block_len: Option<usize>,
    /// 随机种子。同一输入 + 同一 seed 输出逐位一致。默认 42。
    pub seed: u64,
    /// Markdown 中 Top-N 的行数。默认 20。
    pub top: usize,
    /// 参与排名/显著性的最少成交笔数。默认 20。
    pub min_trades: u32,
    /// DSR 的试验数覆盖; `None` 表示用同组同窗口 `status == ok` 的行数。
    /// 必须 >= 1。
    pub trials: Option<usize>,
    /// detail 曲线数上限; 超过则拒绝分析, 避免大网格 OOM。默认 2000。
    pub max_curves: usize,
    /// 要求 detail 必须存在; 缺失时直接报错。默认 false。
    pub require_detail: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            alpha: 0.05,
            bootstrap: 2_000,
            block_len: None,
            seed: 42,
            top: 20,
            min_trades: 20,
            trials: None,
            max_curves: 2_000,
            require_detail: false,
        }
    }
}

/// 纯函数分析的输入: 报告行 + 可选 detail 产物。
#[derive(Debug, Clone)]
pub struct ReportBundle {
    /// `result.parquet` 的解析结果。
    pub rows: Vec<RunResult>,
    /// detail 产物; `None` 表示只做 summary 分析。
    pub detail: Option<vta_report::DetailBundle>,
    /// 输入指纹(FNV-1a), 用于追溯"这份报告是哪一版分析读的"。
    pub input_digest: String,
}

/// 一次分析的完整结果。
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisReport {
    /// JSON schema 版本。
    pub schema_version: u32,
    /// crate 版本。
    pub tool_version: String,
    /// `vta-core` 构建期源码哈希。
    pub source_hash: String,
    /// 输入指纹。
    pub input_digest: String,
    /// 本次分析参数。
    pub options: AnalyzeOptions,
    /// 输入规模与时间范围。
    pub input: InputSummary,
    /// 覆盖与状态统计。
    pub coverage: Coverage,
    /// 分组统计(含参数稳定性与 PBO)。
    pub groups: Vec<GroupAnalysis>,
    /// 全部组合; 排名可用的在前。
    pub combos: Vec<ComboAnalysis>,
    /// Top-N 的 combo id(按 fixed 夏普降序)。
    pub top_ids: Vec<String>,
    /// Top-N 中有 detail 的 combo id(用于回撤/日历/bootstrap 表)。
    pub detail_top_ids: Vec<String>,
    /// 有 detail 的 Top-N 的回撤区间。
    pub drawdowns: Vec<DrawdownRow>,
    /// 有 detail 的 Top-N 的月/年收益。
    pub calendar: Vec<CalendarRow>,
    /// 所有有 detail 的组合的交易结构。
    pub trade_stats: Vec<TradeStatsRow>,
    /// 各组的参数边际。
    pub parameter_marginals: Vec<ParamMarginalRow>,
    /// 硬排除清单(状态、样本不足、对账失败等)。
    pub exclusions: Vec<Exclusion>,
    /// 非致命警告。
    pub warnings: Vec<String>,
}

/// 输入规模与时间范围。
#[derive(Debug, Clone, Serialize)]
pub struct InputSummary {
    /// `result.parquet` 行数。
    pub result_rows: usize,
    /// detail fixed 曲线数; 无 detail 时为 0。
    pub detail_fixed: usize,
    /// detail 复利曲线数。
    pub detail_equity: usize,
    /// detail 逐笔成交组数。
    pub detail_trades: usize,
    /// 是否存在 detail/manifest.toml。
    pub detail_manifest_present: bool,
    /// detail 读取阶段的非致命警告条数(原文见 `warnings`)。
    pub detail_warnings: usize,
    /// 品种列表(排序去重)。
    pub symbols: Vec<String>,
    /// 周期列表(排序去重)。
    pub timeframes: Vec<String>,
    /// 策略列表(排序去重)。
    pub strategies: Vec<String>,
    /// detail 时间轴最早时间戳(ms)。
    pub start_time: Option<i64>,
    /// detail 时间轴最晚时间戳(ms)。
    pub end_time: Option<i64>,
}

/// 覆盖与状态统计。
#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    /// 总行数。
    pub total_rows: usize,
    /// `status == ok` 行数。
    pub ok_rows: usize,
    /// 可参与排名的行数。
    pub rank_eligible: usize,
    /// 可参与显著性分析的行数。
    pub significance_eligible: usize,
    /// detail 对账不一致的行数。
    pub inconsistent: usize,
    /// 缺 detail 的 ok 行数。
    pub detail_missing: usize,
    /// 分组数。
    pub groups: usize,
    /// 状态分布。
    pub status_counts: Vec<CountItem>,
}

/// 一个分组(同品种/周期/策略/执行口径/数据窗口)的汇总。
#[derive(Debug, Clone, Serialize)]
pub struct GroupAnalysis {
    /// 稳定组 id。
    pub id: String,
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 方向口径。
    pub direction: String,
    /// 单边佣金(bp)。
    pub fee_bps: f64,
    /// 单边滑点(bp)。
    pub slippage_bps: f64,
    /// 信号到成交的 bar 间隔。
    pub delay_bars: u32,
    /// 是否按 fill bar 收盘成交。
    pub fill_close: bool,
    /// 数据窗口的 bar 数。
    pub window_bars: u32,
    /// 总行数。
    pub rows: usize,
    /// ok 行数。
    pub ok_rows: usize,
    /// 排名可用行数。
    pub rank_eligible: usize,
    /// 显著性可用行数。
    pub significance_eligible: usize,
    /// 最优参数串。
    pub best_params: Option<String>,
    /// 最优 fixed 夏普。
    pub best_fixed_sharpe: Option<f64>,
    /// fixed 夏普中位数。
    pub median_fixed_sharpe: Option<f64>,
    /// fixed 夏普为正的行占比。
    pub share_positive: Option<f64>,
    /// 参数稳定性摘要。
    pub stability: StabilitySummaryPub,
    /// PBO(组内曲线数/T 不足时为 `None`)。
    pub pbo: Option<PboSummary>,
}

/// 参数稳定性摘要的公开视图(复用内部字段)。
#[derive(Debug, Clone, Serialize)]
pub struct StabilitySummaryPub {
    /// 最优参数串。
    pub best_params: Option<String>,
    /// 最优 fixed 夏普。
    pub best_sharpe: Option<f64>,
    /// 中位 fixed 夏普。
    pub median_sharpe: Option<f64>,
    /// 与最优夏普相差不超过 0.25 的行占比。
    pub share_within_tolerance: Option<f64>,
    /// 最优点的最好邻居夏普。
    pub neighbor_best_sharpe: Option<f64>,
    /// 是否孤立尖峰。
    pub isolated_peak: bool,
}

/// 组内 PBO 摘要。
#[derive(Debug, Clone, Serialize)]
pub struct PboSummary {
    /// 参与 CSCV 的曲线数。
    pub n_strategies: usize,
    /// 截齐后的 bar 数。
    pub t_bars: usize,
    /// 分块数 S。
    pub s_blocks: usize,
    /// φ = PBO。
    pub pbo: f64,
    /// 全样本最优参数串(乐观值)。
    pub full_sample_best_params: String,
    /// 全样本最优 fixed 夏普(乐观值)。
    pub full_sample_best_sharpe: f64,
    /// CSCV 划分下的 IS→OOS 平均衰减。
    pub degradation: f64,
}

/// 单个组合的分析结果。
#[derive(Debug, Clone, Serialize)]
pub struct ComboAnalysis {
    /// 稳定组合 id。
    pub id: String,
    /// 所属组 id。
    pub group_id: String,
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,
    /// 方向口径。
    pub direction: String,
    /// 单边佣金(bp)。
    pub fee_bps: f64,
    /// 单边滑点(bp)。
    pub slippage_bps: f64,
    /// 信号到成交的 bar 间隔。
    pub delay_bars: u32,
    /// 是否按 fill bar 收盘成交。
    pub fill_close: bool,
    /// 数据窗口 bar 数。
    pub window_bars: u32,
    /// 运行状态文本。
    pub status: String,
    /// 成交笔数。
    pub trades: u32,
    /// 实际喂入 bar 数。
    pub bars_used: u32,
    /// fixed 腿总收益(小数)。
    pub fixed_return: f64,
    /// fixed 腿年化夏普。
    pub fixed_sharpe: f64,
    /// fixed 腿最大回撤(小数)。
    pub fixed_max_drawdown: f64,
    /// fixed 腿年化波动率(小数)。
    pub fixed_volatility: Option<f64>,
    /// fixed 腿 Ulcer Index。
    pub fixed_ulcer: Option<f64>,
    /// fixed 腿 95% CVaR(逐 bar 对数收益)。
    pub fixed_cvar_95: Option<f64>,
    /// fixed 腿最差单 bar 对数收益。
    pub fixed_worst_bar: Option<f64>,
    /// 复利腿总收益(小数)。
    pub compound_return: f64,
    /// 复利腿年化夏普。
    pub compound_sharpe: f64,
    /// 复利腿最大回撤(小数)。
    pub compound_max_drawdown: f64,
    /// 复利腿 Ulcer Index。
    pub compound_ulcer: Option<f64>,
    /// 复利腿 95% CVaR。
    pub compound_cvar_95: Option<f64>,
    /// 是否可参与排名。
    pub rank_eligible: bool,
    /// 是否可参与显著性分析。
    pub significance_eligible: bool,
    /// 显著性不可用的原因。
    pub significance_note: Option<String>,
    /// 硬排除原因。
    pub exclusion: Vec<String>,
    /// 与 detail 曲线的对账结果。
    pub reconciliation: Reconciliation,
    /// 显著性结果。
    pub significance: Option<Significance>,
}

/// 单组合对账结果。
#[derive(Debug, Clone, Serialize)]
pub struct Reconciliation {
    /// 是否有 detail。
    pub detail_available: bool,
    /// 是否实际执行了逐项检查。
    pub checked: bool,
    /// 全部检查是否通过。
    pub consistent: bool,
    /// 逐项检查。
    pub checks: Vec<ReconciliationCheck>,
    /// 结构性错误(长度不符/时间轴非递增等)。
    pub errors: Vec<String>,
}

/// 一项引擎列与曲线复算的对照。
#[derive(Debug, Clone, Serialize)]
pub struct ReconciliationCheck {
    /// 指标名。
    pub name: String,
    /// 引擎列的值。
    pub engine: f64,
    /// 曲线/成交复算的值。
    pub recomputed: f64,
    /// 绝对差。
    pub abs_diff: f64,
    /// 容差。
    pub tolerance: f64,
    /// 是否在容差内。
    pub ok: bool,
}

/// 单组合显著性。
#[derive(Debug, Clone, Serialize)]
pub struct Significance {
    /// 收益笔数 T。
    pub n_returns: usize,
    /// 逐 bar 夏普。
    pub sr_bar: f64,
    /// 年化夏普。
    pub sr_annual: f64,
    /// 收益偏度。
    pub skew: f64,
    /// 收益原始峰度。
    pub kurtosis: f64,
    /// Lo (2002) 年化标准误。
    pub lo_se_annual: Option<f64>,
    /// PSR(0)。
    pub psr_zero: Option<f64>,
    /// `1 - PSR(0)`。
    pub psr_p_value: Option<f64>,
    /// 达到 `alpha` 所需的最小收益笔数 + 1(bar 数)。
    pub min_trl_bars: Option<f64>,
    /// 实际 bar 数是否已达到 MinTRL。
    pub min_trl_ok: Option<bool>,
    /// DSR。
    pub dsr: Option<f64>,
    /// DSR 使用的试验数 N。
    pub trials: usize,
    /// 同组试验的逐 bar 夏普方差。
    pub trial_sr_variance: Option<f64>,
    /// DSR 的期望最大夏普(年化)。
    pub expected_max_sr_annual: Option<f64>,
    /// 平稳 bootstrap 置信区间。
    pub bootstrap: Option<BootstrapResult>,
    /// 同组 BH q 值。
    pub bh_q: Option<f64>,
    /// BH 的组内检验数。
    pub bh_group_size: Option<usize>,
}

/// 一条回撤区间(仅有 detail 的 Top-N)。
#[derive(Debug, Clone, Serialize)]
pub struct DrawdownRow {
    /// 组合 id。
    pub combo_id: String,
    /// 全量可排名组合中的名次(1 起; 缺 detail 的更高名次会留空)。
    pub rank: usize,
    /// 峰值时间戳(ms)。
    pub peak_time: i64,
    /// 谷底时间戳(ms)。
    pub trough_time: i64,
    /// 恢复时间戳(ms); 未恢复为 `None`。
    pub recovery_time: Option<i64>,
    /// 回撤深度(小数)。
    pub depth: f64,
    /// 峰值到谷底 bar 数。
    pub peak_to_trough_bars: usize,
    /// 谷底到恢复 bar 数。
    pub recovery_bars: Option<usize>,
    /// 总水下 bar 数。
    pub total_bars: usize,
}

/// 一条日历收益(仅有 detail 的 Top-N)。
#[derive(Debug, Clone, Serialize)]
pub struct CalendarRow {
    /// 组合 id。
    pub combo_id: String,
    /// `YYYY-MM` 或 `YYYY`。
    pub period: String,
    /// 周期起始时间戳(ms)。
    pub start_time: i64,
    /// 周期结束时间戳(ms)。
    pub end_time: i64,
    /// 周期收益(小数)。
    pub return_ratio: f64,
    /// 周期 bar 数。
    pub bars: usize,
    /// 周期内最大回撤(小数)。
    pub max_drawdown: f64,
}

/// 一个组合的交易结构行。
#[derive(Debug, Clone, Serialize)]
pub struct TradeStatsRow {
    /// 组合 id。
    pub combo_id: String,
    /// 交易结构统计。
    pub stats: TradeStats,
}

/// 一组参数取值的边际。
#[derive(Debug, Clone, Serialize)]
pub struct ParamMarginalRow {
    /// 组 id。
    pub group_id: String,
    /// 参数名。
    pub param: String,
    /// 参数值原文。
    pub value: String,
    /// 解析出的数值。
    pub numeric: Option<f64>,
    /// 行数。
    pub n: usize,
    /// 平均 fixed 夏普。
    pub mean_sharpe: f64,
    /// 中位 fixed 夏普。
    pub median_sharpe: f64,
    /// fixed 夏普标准差。
    pub std_sharpe: f64,
    /// 平均 fixed 收益。
    pub mean_return: f64,
}

/// 一条硬排除记录。
#[derive(Debug, Clone, Serialize)]
pub struct Exclusion {
    /// 组合 id。
    pub id: String,
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串。
    pub params: String,
    /// 运行状态文本。
    pub status: String,
    /// 排除原因。
    pub reason: String,
}
