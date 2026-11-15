//! # vta-analysis — 报告只读分析层
//!
//! 读取 `vta` 已产出的 `result.parquet` 与可选 `detail/` 产物, 按专业文献
//! 口径做对账、描述性统计、回撤、交易结构、日历、参数稳定性与显著性检验。
//!
//! ## 边界
//!
//! - **只读**: 不重跑回测, 不修改 `result.parquet` / `detail/` 下任何文件;
//! - **引擎指标是权威值**: 曲线复算只用于对账, 偏差超过容差的行标记
//!   `reconciliation_failed` 并排除出排名;
//! - **不跨口径混排**: 分组键包含 symbol / timeframe / strategy / direction /
//!   fee / slippage / delay / fill_close / 数据窗口; 不同组不合并排序。
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | `model` | 公开数据模型与 `analysis.json` schema |
//! | `analyze` | 主编排: 对账、分组、排名、显著性、PBO |
//! | `input` | 报告目录装载与输入指纹 |
//! | `render` | Markdown / JSON / CSV 输出 |
//!
//! ## 参考文献
//!
//! - Bailey, D. H. & López de Prado, M. (2012). The Sharpe Ratio Efficient
//!   Frontier. *Journal of Risk* 15(2).
//! - Bailey, D. H. & López de Prado, M. (2014). The Deflated Sharpe Ratio.
//!   *Journal of Portfolio Management* 40(5).
//! - Bailey, Borwein, López de Prado & Zhu (2016). The Probability of
//!   Backtest Overfitting. *Journal of Computational Finance* 20(4).
//! - Lo, A. W. (2002). The Statistics of Sharpe Ratios. *Financial Analysts
//!   Journal* 58(4).
//! - Politis, D. N. & Romano, J. P. (1994). The Stationary Bootstrap.
//!   *JASA* 89(428).
//! - Politis, D. N. & White, H. (2004). Automatic Block-Length Selection for
//!   the Dependent Bootstrap. *Econometric Reviews* 23(1).
//! - Benjamini, Y. & Hochberg, Y. (1995). Controlling the False Discovery
//!   Rate. *JRSS-B* 57(1).
//! - Martin, P. & McCann, B. (1989). *The Investor's Guide to Fidelity Funds*.
//!
//! ## 快速上手
//!
//! ```no_run
//! # fn main() -> vta_core::error::Result<()> {
//! let opts = vta_analysis::AnalyzeOptions::default();
//! let report = vta_analysis::analyze(std::path::Path::new("reports/latest"), &opts)?;
//! vta_analysis::write_report(&report, std::path::Path::new("reports/latest/analysis"))?;
//! # Ok(())
//! # }
//! ```

mod analyze;
mod bootstrap;
mod calendar;
mod drawdown;
mod input;
mod metrics;
mod model;
mod render;
mod stability;
mod stats;
mod trades;

pub use analyze::{analyze, analyze_bundle};
pub use bootstrap::BootstrapResult;
pub use calendar::{CalendarSummary, PeriodReturn};
pub use drawdown::{DrawdownEpisode, DrawdownSummary};
pub use model::{
    AnalysisReport, AnalyzeOptions, CalendarRow, ComboAnalysis, Coverage, DrawdownRow, Exclusion,
    GroupAnalysis, InputSummary, ParamMarginalRow, PboSummary, Reconciliation, ReconciliationCheck,
    ReportBundle, SCHEMA_VERSION, Significance, StabilitySummaryPub, TradeStatsRow,
};
pub use render::write_report;
pub use stability::{ParamMarginal, StabilitySummary};
pub use trades::{CountItem, TradeStats};
