//! 逐笔明细与权益曲线的落盘、清单与缓存校验。

mod manifest;
mod read;
mod write;

pub use manifest::{
    DETAIL_INDEX_FILE, detail_cache_is_complete, invalidate_combo_detail_manifest,
    write_combo_detail_manifest, write_detail_index,
};
pub use read::{
    EquityCurve, FixedCurve, col_by_name, read_equity_curves, read_fixed_curves,
    read_fixed_curves_from_manifest,
};
pub use write::write_detail_parquet;
