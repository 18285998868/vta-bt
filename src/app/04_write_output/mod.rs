/// Parquet 读写层: 报告落盘、缓存校验读取、曲线读取与合并。
pub mod parquet;

pub use parquet::{EquityCurve, FixedCurve, MergeStats};
pub use parquet::{
    merge_parquet_files, params_to_json, parquet_row_count, read_equity_curves, read_fixed_curves,
    read_parquet, strategy_params_value, write_detail_parquet, write_parquet, write_single_parquet,
};
