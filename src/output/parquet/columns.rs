//! Arrow 列读取的通用辅助: 取列名、类型检查与非空提取。

use std::path::Path;

use arrow::array::*;
use arrow::record_batch::RecordBatch;

use crate::error::Result;
use crate::money::MONEY_SCALE;

use super::schema::ColumnIndex;

/// 列类型不符时的错误消息。
///
/// 可触发场景: `read_parquet` 读的不只是本次写的文件, 还有**历史版本**写的缓存、
/// 别的工具产出、或被手工改过的文件。列一旦类型不符(例如 `total_return` 是
/// Int32 而非 Float64), 原实现直接 panic —— 整轮回测崩在**第一个组合**上,
/// 用户只看到一句 "f64 column", 完全不知道是哪个文件、哪一列出的问题。
pub(super) fn col_type_err(
    file: &Path,
    name: &str,
    expect: &str,
    actual: &str,
) -> crate::error::AppError {
    crate::error::AppError::Msg(format!(
        "{}: 列 {name} 类型不符(期望 {expect}, 实际 {actual}); \
         该文件可能由其它版本或工具产出, 请删掉后重跑",
        file.display()
    ))
}

/// 按列名取列的下标(供类型校验用)。
pub(super) fn col_name(batch: &RecordBatch, idx: usize) -> String {
    batch
        .schema()
        .fields()
        .get(idx)
        .map_or_else(|| format!("#{idx}"), |f| f.name().clone())
}

/// 列中出现 null 时的错误。
///
/// null 槽位在 `Array::value(i)` 下会**静默**变成该类型的默认值
/// (`Float64` → `0.0`, `Utf8` → `""`, `UInt*` → `0`), 而本项目的报告 schema
/// 全部 `nullable = false` —— 所以 null 只可能来自"别的工具写的文件"或
/// "被手工改过的文件"。那种文件被静默读成默认值时:
/// - `sharpe` 的 null → `0.0` → 与"真实零波动低风险"**不可区分**(这正是
///   `RunStatus` 存在的理由);
/// - `status` 的 null → `""` → 落到 `_ => RunStatus::NoSignals` 分支, 把
///   "正常"误判成"退化"。
///
/// 故 null 一律**报错**, 不静默填默认值。
pub(super) fn null_err(file: &Path, name: &str, row: usize) -> crate::error::AppError {
    crate::error::AppError::Msg(format!(
        "{}: 列 {name} 第 {row} 行为 null —— 该 schema 声明为非空; \
         文件可能由其它工具产出或被手工改写, 请删掉后重跑",
        file.display()
    ))
}

/// 读一列的**非空**值: null 槽位报错, 不静默填默认值。
pub(super) fn read_non_null<T, A>(
    batch: &RecordBatch,
    idx: usize,
    file: &Path,
    arr: &A,
    get: impl Fn(&A, usize) -> T,
) -> Result<Vec<T>>
where
    A: Array,
{
    (0..batch.num_rows())
        .map(|i| {
            if arr.is_null(i) {
                return Err(null_err(file, &col_name(batch, idx), i));
            }
            Ok(get(arr, i))
        })
        .collect()
}

pub(super) fn str_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<String>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Utf8", "非字符串"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i).to_string())
}

pub(super) fn i64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<i64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Int64", "非 Int64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

pub(super) fn u64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt64", "非 UInt64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

/// 读一列 `Decimal128` 金额(定点十进制)。
pub(super) fn dec_col(
    batch: &RecordBatch,
    idx: usize,
    file: &Path,
) -> Result<Vec<crate::money::Amount>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Decimal128", "非 Decimal128"))?;
    read_non_null(batch, idx, file, arr, |a, i| {
        crate::money::Amount::from_decimal128_mantissa(a.value(i), MONEY_SCALE)
    })
}

/// 读一列**可选**的 `Decimal128` 金额(旧缓存无此列 ⇒ 缺省零)。
pub(super) fn opt_dec(
    batch: &RecordBatch,
    ci: &ColumnIndex,
    name: &str,
    n: usize,
    file: &Path,
) -> Result<Vec<crate::money::Amount>> {
    match ci.opt(name) {
        Some(i) => dec_col(batch, i, file),
        None => Ok(vec![crate::money::Amount::ZERO; n]),
    }
}

pub(super) fn f64_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<f64>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "Float64", "非 Float64"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

pub(super) fn u32_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u32>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt32", "非 UInt32"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}

pub(super) fn u8_col(batch: &RecordBatch, idx: usize, file: &Path) -> Result<Vec<u8>> {
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or_else(|| col_type_err(file, &col_name(batch, idx), "UInt8", "非 UInt8"))?;
    read_non_null(batch, idx, file, arr, |a, i| a.value(i))
}
