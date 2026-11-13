//! 报告列的声明式单位表与列索引。

use std::path::{Path, PathBuf};

use crate::error::Result;

/// 报告列的**单位**。
///
/// 这张 enum 存在的理由是具体的缺陷, 不是风格偏好:
/// - `mdd` 落盘时**除以了 100**(小数), 而同一张表里的 `fixed_max_drawdown_pct`
///   **没有除**(百分数)。两条腿并排放, 读者无从判断谁是百分数 —— 这是会直接
///   导致结论出错的单位错配。
/// - `volatility` 是百分数, `max_drawdown`/`win_rate` 是小数, 而三个列名里
///   都没有任何单位标记。
///
/// 单位在落盘层是**声明式**的: 见 [`COLUMNS`]。新增列若不声明单位, `cargo test`
/// 会失败, 而不是默默用错单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// 无量纲比率, **小数**: 0.15 = 15%。
    Ratio,
    /// 百分数: 15.0 = 15%。
    Percent,
    /// 货币(以初始资金计价)。
    Money,
    /// 价格差(报价货币), 非货币金额。
    Price,
    /// bar 数。
    Bars,
    /// 次 / 年。
    PerYear,
    /// 整数计数。
    Count,
    /// 基点(bp): 2.0 = 2bp = 0.02%。
    BasisPoints,
    /// 无量纲且非比率(Sharpe / 偏度 / p 值等)。
    Dimensionless,
}

/// 报告列的单位契约。**schema 的单一来源**: 列名 + 单位一一对应。
///
/// 与之配套的换算约定(落盘时):
/// 换算约定(落盘时):
///
/// - `Ratio` 类: `SummaryMetrics` 的 `_pct` 字段**除以 100** 后写入
/// - `Percent` 类: 直接取 `SummaryMetrics` 的 `_pct` 字段
/// - 其余: 按 `SummaryMetrics` 原值写入
///
/// `test_every_report_column_declares_its_unit` 保证本表与 schema 完全一致,
/// 少声明一列就会测试失败, 而不是默默用错单位。
pub const COLUMNS: &[(&str, Unit)] = &[
    ("strategy", Unit::Dimensionless),
    ("symbol", Unit::Dimensionless),
    ("timeframe", Unit::Dimensionless),
    ("strategy_params", Unit::Dimensionless),
    ("strategy_params_json", Unit::Dimensionless),
    ("balance", Unit::Money),
    ("trade_size", Unit::Dimensionless),
    ("total_return", Unit::Ratio),
    ("final_equity", Unit::Money),
    ("max_drawdown", Unit::Ratio),
    ("trades", Unit::Count),
    ("stop_loss_exits", Unit::Count),
    ("trailing_stop_exits", Unit::Count),
    ("strategy_stop_exits", Unit::Count),
    ("sharpe", Unit::Dimensionless),
    ("sortino", Unit::Dimensionless),
    ("win_rate", Unit::Ratio),
    ("profit_factor", Unit::Dimensionless),
    ("calmar", Unit::Dimensionless),
    ("avg_win", Unit::Money),
    ("avg_loss", Unit::Money),
    // `expectancy` 是**逐笔 pnl_pct 的均值**(比率), 不是货币金额 ——
    // 声明成 `Money` 会让读者把它与 `avg_win`/`avg_loss` 混为一谈。
    ("expectancy", Unit::Ratio),
    ("consecutive_wins", Unit::Count),
    ("consecutive_losses", Unit::Count),
    ("volatility_pct", Unit::Percent),
    ("recovery_factor", Unit::Dimensionless),
    ("avg_hold_bars", Unit::Bars),
    ("avg_mae_price", Unit::Price),
    ("avg_mfe_price", Unit::Price),
    ("edge_ratio", Unit::Dimensionless),
    ("wl_ratio", Unit::Dimensionless),
    ("skewness", Unit::Dimensionless),
    ("kurtosis", Unit::Dimensionless),
    ("sqn", Unit::Dimensionless),
    ("pf_stability", Unit::Dimensionless),
    ("z_score", Unit::Dimensionless),
    ("p_value", Unit::Dimensionless),
    ("fixed_pnl", Unit::Money),
    ("params", Unit::Dimensionless),
    ("turnover_per_year", Unit::PerYear),
    ("delay_bars", Unit::Count),
    ("fee_bps", Unit::BasisPoints),
    ("slippage_bps", Unit::BasisPoints),
    ("fixed_total_return", Unit::Ratio),
    ("fixed_sharpe_ratio", Unit::Dimensionless),
    // 单位修正: 原名 fixed_max_drawdown_pct, 值是**百分数**, 而复利腿的
    // max_drawdown 是**小数**。同名不同量纲是最容易读错的一种不一致。
    ("fixed_max_drawdown", Unit::Ratio),
    // 单位是 **bp**(基点), 不是货币: 1.0 = 1bp = 0.01%。
    ("fixed_expectancy_bps", Unit::BasisPoints),
    ("fixed_profit_factor", Unit::Dimensionless),
    ("fill_close", Unit::Dimensionless),
    ("buy_hold_pnl", Unit::Money),
    ("buy_hold_return", Unit::Ratio),
    ("bars_used", Unit::Count),
    ("status", Unit::Dimensionless),
];

/// 按**列名**索引 parquet 列。取代原先 40 个魔法列号。
///
/// 为什么必须按名: `read_parquet` 既读**本次写的**文件也读**历史版本写的**缓存。
/// 用常量下标读取时, 只要列的顺序或数量变化(每追加一列就是一次), 下标就可能
/// 落到类型恰好兼容的另一列上 —— 不报错, 数值**静默错位**(例如把 `sharpe`
/// 读成 `sortino`, 两者都是 `f64`)。按名读取 + 缺列时列出实际列名, 让错配在
/// 第一次发生时就炸掉。
///
/// - [`ColumnIndex::req`] 必须存在, 缺则 `Err` 并列出实际列名。
/// - `ColumnIndex::opt` 可选(旧缓存文件没有该列), 由调用方决定回退值。
#[derive(Debug, Clone)]
pub(super) struct ColumnIndex {
    map: std::collections::HashMap<String, usize>,
    file: PathBuf,
}

impl ColumnIndex {
    pub(super) fn new(schema: &arrow::datatypes::Schema, file: &Path) -> Self {
        Self {
            map: schema
                .fields()
                .iter()
                .enumerate()
                .map(|(i, f)| (f.name().clone(), i))
                .collect(),
            file: file.to_path_buf(),
        }
    }

    pub(super) fn req(&self, name: &str) -> Result<usize> {
        self.map.get(name).copied().ok_or_else(|| {
            let mut actual: Vec<&str> = self.map.keys().map(String::as_str).collect();
            actual.sort_unstable();
            crate::error::AppError::Msg(format!(
                "{} 缺必需列 {name:?}; 实际列: {actual:?}(旧格式缓存请删 temps/ 后重跑)",
                self.file.display()
            ))
        })
    }

    pub(super) fn opt(&self, name: &str) -> Option<usize> {
        self.map.get(name).copied()
    }
}
