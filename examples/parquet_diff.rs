//! `parquet_diff`: 逐值比较两个报告 parquet, 供重构的"行为不变"验收使用。
//!
//! ```bash
//! cargo run --release --example parquet_diff -- a/result.parquet b/result.parquet
//! ```
//!
//! 比较口径:
//! - schema: 列名、列序、`DataType` 必须完全一致;
//! - 行数一致;
//! - 每个单元格按**类型精确**比较: `f64` 比 `to_bits`(区分 `-0.0`, `NaN` 归一),
//!   `Decimal128` 比原始 `i128` 与标度, 字符串/整数直接比。
//!
//! 不做的事: 不比较 parquet 文件字节(压缩与 writer 元数据会让相同内容产生
//! 不同字节), 不做容差比较(容差会掩盖"重构顺手改了口径")。
//!
//! 退出码: 0 = 完全一致; 1 = 存在差异(差异明细打到 stderr)。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use arrow::array::{
    Array, ArrayRef, BooleanArray, Decimal128Array, Float32Array, Float64Array, Int16Array,
    Int32Array, Int64Array, StringArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow::datatypes::{DataType, SchemaRef};
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// 读取一个 parquet 的全部 batch 与 schema。
fn read_all(path: &Path) -> (SchemaRef, Vec<RecordBatch>) {
    let file = std::fs::File::open(path).expect("文件应可打开");
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).expect("应可解析 parquet 头");
    let schema = builder.schema().clone();
    let batches = builder
        .build()
        .expect("应可构建 reader")
        .collect::<Result<Vec<_>, _>>()
        .expect("应可读取全部 batch");
    (schema, batches)
}

/// 把一行一列渲染成可比较的文本(类型精确)。
fn cell(array: &ArrayRef, row: usize) -> String {
    if array.is_null(row) {
        return "<null>".to_string();
    }
    match array.data_type() {
        DataType::Utf8 => array
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("Utf8 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::UInt8 => array
            .as_any()
            .downcast_ref::<UInt8Array>()
            .expect("UInt8 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::UInt16 => array
            .as_any()
            .downcast_ref::<UInt16Array>()
            .expect("UInt16 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::UInt32 => array
            .as_any()
            .downcast_ref::<UInt32Array>()
            .expect("UInt32 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::UInt64 => array
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("UInt64 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::Int16 => array
            .as_any()
            .downcast_ref::<Int16Array>()
            .expect("Int16 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::Int32 => array
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("Int32 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::Int64 => array
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("Int64 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::Boolean => array
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("Boolean 列应可下行转型")
            .value(row)
            .to_string(),
        DataType::Float32 => {
            let v = array
                .as_any()
                .downcast_ref::<Float32Array>()
                .expect("Float32 列应可下行转型")
                .value(row);
            if v.is_nan() {
                "NaN".to_string()
            } else {
                format!("{:08x}", v.to_bits())
            }
        }
        DataType::Float64 => {
            // 比 bit 模式: `-0.0` 与 `0.0` 不同, 全部 NaN 归一到同一文本。
            let v = array
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("Float64 列应可下行转型")
                .value(row);
            if v.is_nan() {
                "NaN".to_string()
            } else {
                format!("{:016x}", v.to_bits())
            }
        }
        DataType::Decimal128(_, scale) => format!(
            "{}e{}",
            array
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .expect("Decimal128 列应可下行转型")
                .value(row),
            scale
        ),
        other => panic!("parquet_diff 未支持的类型: {other}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("用法: parquet_diff <left.parquet> <right.parquet>".into());
    }
    let (ls, lb) = read_all(Path::new(&args[0]));
    let (rs, rb) = read_all(Path::new(&args[1]));

    let mut diffs = 0usize;
    let mut report = |msg: String| {
        if diffs < 40 {
            eprintln!("{msg}");
        }
        diffs += 1;
    };

    if ls.fields().len() != rs.fields().len() {
        report(format!(
            "列数不同: {} vs {}",
            ls.fields().len(),
            rs.fields().len()
        ));
    }
    for (l, r) in ls.fields().iter().zip(rs.fields().iter()) {
        if l.name() != r.name() {
            report(format!("列名不同: {} vs {}", l.name(), r.name()));
        }
        if l.data_type() != r.data_type() {
            report(format!(
                "列 {} 类型不同: {:?} vs {:?}",
                l.name(),
                l.data_type(),
                r.data_type()
            ));
        }
    }

    let lrows: usize = lb.iter().map(RecordBatch::num_rows).sum();
    let rrows: usize = rb.iter().map(RecordBatch::num_rows).sum();
    if lrows != rrows {
        report(format!("行数不同: {lrows} vs {rrows}"));
    }

    let width = ls.fields().len().min(rs.fields().len());
    for col in 0..width {
        let name = ls.field(col).name().clone();
        let mut lrow = 0usize;
        for lbatch in &lb {
            let larr: &ArrayRef = lbatch.column(col);
            for i in 0..larr.len() {
                // 两端 batch 划分可能不同, 故按全局行号定位右侧单元格。
                let Some((rarr, ri)) = locate(&rb, col, lrow + i) else {
                    break;
                };
                let a = cell(larr, i);
                let b = cell(rarr, ri);
                if a != b {
                    report(format!("列 {name} 第 {} 行: {a} vs {b}", lrow + i));
                }
            }
            lrow += lbatch.num_rows();
        }
    }

    if diffs == 0 {
        println!("一致: {} 列 × {} 行", width, lrows);
        Ok(())
    } else {
        Err(format!("{diffs} 处不同(仅显示前 40 条)").into())
    }
}

/// 在 batch 列表中定位全局第 `row` 行(跨 batch 的行号).
fn locate(batches: &[RecordBatch], col: usize, row: usize) -> Option<(&ArrayRef, usize)> {
    let mut start = 0usize;
    for b in batches {
        let end = start + b.num_rows();
        if row < end {
            return Some((b.column(col), row - start));
        }
        start = end;
    }
    None
}
