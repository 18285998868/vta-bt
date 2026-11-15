//! Parquet 读写层: 报告落盘、缓存校验读取、曲线读取与合并。
//!
//! | 模块 | 职责 |
//! |---|---|
//! | `report` | 汇总报告构建与原子写入 |
//! | `detail` | 逐笔明细/权益曲线落盘与缓存清单 |
//! | `columns` | Arrow 列读取辅助 |
//! | `schema` | 单位表与列索引 |
//! | `read` | 报告/曲线读取、合并 |
//!
//! 参数串编码(`param_str` 的逆运算)住在 `vta-strategies`, 因为它必须认识
//! 各策略的参数形态与均线名单。

mod columns;
mod detail;
mod read;
mod report;
mod schema;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use detail::{
    DETAIL_INDEX_FILE, EquityCurve, FixedCurve, detail_cache_is_complete,
    invalidate_combo_detail_manifest, read_equity_curves, read_fixed_curves,
    read_fixed_curves_from_manifest, write_combo_detail_manifest, write_detail_index,
    write_detail_parquet,
};
pub use read::{
    MergeStats, merge_parquet_files, parquet_row_count, read_parquet, write_single_parquet,
};
pub use report::write_parquet;
pub use schema::{COLUMNS, Unit};

/// 按列名取索引(供 `detail`/`read` 共用)。
pub use detail::col_by_name;
