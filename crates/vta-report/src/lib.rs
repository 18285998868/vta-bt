//! # vta-report — Parquet 报告层
//!
//! 报告 schema、列单位契约、逐值读写、detail 清单与合并。所有函数围绕
//! `vta-core` 的 [`RunResult`](vta_core::RunResult) / [`DetailResult`](vta_core::DetailResult)
//! 工作, 不依赖策略实现与 CLI。
//!
//! 参数串编码(`param_str` 的逆运算)住在 `vta-strategies`, 因为它必须认识
//! 各策略的参数形态。

/// Parquet 读写层: 报告落盘、缓存校验读取、曲线读取与合并。
pub mod parquet;

pub use parquet::{DETAIL_INDEX_FILE, EquityCurve, FixedCurve, MergeStats};
pub use parquet::{
    detail_cache_is_complete, invalidate_combo_detail_manifest, merge_parquet_files,
    parquet_row_count, read_equity_curves, read_fixed_curves, read_fixed_curves_from_manifest,
    read_parquet, write_combo_detail_manifest, write_detail_index, write_detail_parquet,
    write_parquet, write_single_parquet,
};
