//! 逐笔明细与权益曲线的落盘、清单与缓存校验。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field};
use arrow::record_batch::RecordBatch;

use crate::error::Result;

use super::super::report::{
    MONEY_PRECISION, MONEY_SCALE_I8, money_builder, money_mantissa, sanitize_params_for_filename,
    write_atomic,
};

/// 写出 `--detail` 的产物: 逐笔交易 + 复利曲线 + (可选) fixed 曲线。
///
/// 每组 (symbol, timeframe, strategy, params) 一个文件组。`write_curves` 为 false
/// 时跳过 GB 级的 fixed 曲线 I/O(做 PBO 时才需开启)。
///
/// 每条曲线/交易文件的 schema metadata 都带 `combo`(产出它的组合缓存 id)与
/// `status`(该档位运行状态); fixed 曲线另用 `time` 列而非下标, 使 PBO 只能
/// 对齐**同一时间网格**的曲线。返回本次实际写出的文件路径, 供调用方写侧车清单。
///
/// # Errors
/// 目录创建、文件写入或 Arrow 构造失败时返回 `AppError`(crate::error::AppError)。
pub fn write_detail_parquet(
    details: &[crate::app::DetailResult],
    dir: &Path,
    write_curves: bool,
    combo: &str,
) -> Result<Vec<PathBuf>> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::collections::HashMap;
    use std::fs::File;

    let path = dir;
    std::fs::create_dir_all(path)?;
    let mut written = Vec::new();

    let mut groups: HashMap<(String, String, String, String), Vec<&crate::app::DetailResult>> =
        HashMap::new();
    for d in details {
        groups
            .entry((
                d.symbol.clone(),
                d.timeframe.clone(),
                d.strategy.clone(),
                d.params.clone(),
            ))
            .or_default()
            .push(d);
    }

    for ((symbol, tf, strategy, params), group) in &groups {
        let clean_params = sanitize_params_for_filename(params);
        let filename = format!("{}_{}_{}_{}.parquet", symbol, tf, strategy, clean_params);
        let filepath = path.join(filename);

        // 同一 params 组只能有一个状态。若出现两个状态, 说明调用方把不同
        // (symbol, tf, strategy, params) 的批次混在一起, 后续 PBO 无法判断
        // 该曲线该不该参与 —— 直接报错, 不静默取第一个。
        let status = group
            .first()
            .map_or(crate::app::RunStatus::Ok, |d| d.status);
        if let Some(bad) = group.iter().find(|d| d.status != status) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 分组 {symbol}/{tf}/{strategy}/{params} 内 status 不一致: {} vs {}",
                status.as_str(),
                bad.status.as_str()
            )));
        }

        // 分组元数据进 schema metadata, 文件名含 params 明文(含 `_` 清洗)故不
        // 做反向解析。两条曲线的 stem 相同, 只有 kind 后缀不同。
        // `combo` 让缓存命中校验能发现"同名文件被另一轮覆盖"; `status` 让
        // `--pbo` 能显式筛掉 no_signals/no_trades/bankrupt。
        // trades 文件也写同一份 metadata: 侧车清单的完整性校验会遍历全部产物。
        let curve_meta = arrow::datatypes::Metadata::new()
            .with("combo", combo.to_string())
            .with("status", status.as_str().to_string())
            .with("symbol", symbol.clone())
            .with("timeframe", tf.clone())
            .with("strategy", strategy.clone())
            .with("params", params.clone());

        let mut trade_entry_time = Int64Builder::new();
        let mut trade_exit_time = Int64Builder::new();
        let mut trade_side = StringBuilder::new();
        let mut trade_entry = Float64Builder::new();
        let mut trade_exit = Float64Builder::new();
        let mut trade_qty = money_builder()?;
        let mut trade_pnl = money_builder()?;
        let mut trade_pnl_pct = Float64Builder::new();
        let mut trade_fixed_pnl = money_builder()?;
        let mut trade_bars_held = Int64Builder::new();
        let mut trade_max_runup = Float64Builder::new();
        let mut trade_max_drawdown = Float64Builder::new();
        let mut trade_exit_reason = StringBuilder::new();

        for d in group {
            for t in &d.trades {
                trade_entry_time.append_value(t.entry_time);
                trade_exit_time.append_value(t.exit_time);
                trade_side.append_value(t.side.as_str());
                trade_entry.append_value(t.entry_price);
                trade_exit.append_value(t.exit_price);
                trade_qty.append_value(money_mantissa(t.qty)?);
                trade_pnl.append_value(money_mantissa(t.pnl)?);
                trade_pnl_pct.append_value(t.pnl_pct);
                trade_fixed_pnl.append_value(money_mantissa(t.fixed_pnl)?);
                trade_bars_held.append_value(t.bars_held);
                trade_max_runup.append_value(t.max_runup);
                trade_max_drawdown.append_value(t.max_drawdown);
                trade_exit_reason.append_value(t.exit_reason);
            }
        }

        let trade_cols: Vec<ArrayRef> = vec![
            Arc::new(trade_entry_time.finish()),
            Arc::new(trade_exit_time.finish()),
            Arc::new(trade_side.finish()),
            Arc::new(trade_entry.finish()),
            Arc::new(trade_exit.finish()),
            Arc::new(trade_qty.finish()),
            Arc::new(trade_pnl.finish()),
            Arc::new(trade_pnl_pct.finish()),
            Arc::new(trade_fixed_pnl.finish()),
            Arc::new(trade_bars_held.finish()),
            Arc::new(trade_max_runup.finish()),
            Arc::new(trade_max_drawdown.finish()),
            Arc::new(trade_exit_reason.finish()),
        ];

        let trade_fields = vec![
            Field::new("entry_time", DataType::Int64, false),
            Field::new("exit_time", DataType::Int64, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("entry_price", DataType::Float64, false),
            Field::new("exit_price", DataType::Float64, false),
            Field::new(
                "qty",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new(
                "pnl",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new("pnl_pct", DataType::Float64, false),
            Field::new(
                "fixed_pnl",
                DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
                false,
            ),
            Field::new("bars_held", DataType::Int64, false),
            Field::new("max_runup", DataType::Float64, false),
            Field::new("max_drawdown", DataType::Float64, false),
            Field::new("exit_reason", DataType::Utf8, false),
        ];

        let trade_schema =
            arrow::datatypes::Schema::new_with_metadata(trade_fields, curve_meta.clone());
        let trade_batch = RecordBatch::try_new(Arc::new(trade_schema), trade_cols)?;

        write_atomic(&filepath, |tmp| {
            let file = File::create(tmp)?;
            let props = WriterProperties::builder().build();
            let mut writer = ArrowWriter::try_new(file, trade_batch.schema(), Some(props))?;
            writer.write(&trade_batch)?;
            writer.close()?;
            Ok(())
        })?;
        written.push(filepath);

        let stem = format!("{}_{}_{}_{}", symbol, tf, strategy, clean_params);

        // 复利腿逐 bar 权益。`docs/configuration.md` 承诺 `detail/` 导出"逐笔交易/权益曲线",
        // 但历史上**只写了 trades**: 权益曲线从未落盘, 复算需重跑整轮回测。
        let mut bar_time = Int64Builder::new();
        let mut equity = Float64Builder::new();
        for d in group {
            for &(ts, eq) in &d.equity_curve {
                bar_time.append_value(ts as i64);
                equity.append_value(eq);
            }
        }
        written.push(write_curve_file(
            path,
            &stem,
            "equitycurve",
            &curve_meta,
            Field::new("time", DataType::Int64, false),
            Arc::new(bar_time.finish()),
            Field::new("equity", DataType::Float64, false),
            Arc::new(equity.finish()),
        )?);

        // fixed 腿逐 bar 权益: `--pbo` 的 T×N 输入。
        // `write_curves=false`（默认）时跳过: GB 级 I/O 只在做 PBO 时花。
        if !write_curves {
            continue;
        }
        let mut fixed_time = Int64Builder::new();
        let mut fixed_equity = Float64Builder::new();
        for d in group {
            if d.equity_curve.len() != d.fixed_curve.len() {
                return Err(crate::error::AppError::Msg(format!(
                    "detail 分组 {symbol}/{tf}/{strategy}/{params}: equity_curve {} 行 != fixed_curve {} 行",
                    d.equity_curve.len(),
                    d.fixed_curve.len()
                )));
            }
            // 时间必须取自 equity_curve 的同一根 bar; 只写 bar_idx 时,
            // 不同数据窗口的同名参数文件在 PBO 里无法区分, 只能静默按
            // `min(len)` 截断。
            for (&(ts, _), value) in d.equity_curve.iter().zip(d.fixed_curve.iter()) {
                fixed_time.append_value(ts as i64);
                fixed_equity.append_value(*value);
            }
        }
        written.push(write_curve_file(
            path,
            &stem,
            "fixedcurve",
            &curve_meta,
            Field::new("time", DataType::Int64, false),
            Arc::new(fixed_time.finish()),
            Field::new("fixed_equity", DataType::Float64, false),
            Arc::new(fixed_equity.finish()),
        )?);
    }

    Ok(written)
}

/// 写一条逐 bar 曲线到 `{dir}/{stem}_{kind}.parquet`。
///
/// `kind` 为 `equitycurve` / `fixedcurve`; 两条曲线的**列语义不同**, 故索引列与
/// 值列由调用方给出而不共用一套 schema(避免"列名对但含义错"的静默误读)。
#[allow(clippy::too_many_arguments)]
pub(super) fn write_curve_file(
    dir: &std::path::Path,
    stem: &str,
    kind: &str,
    meta: &arrow::datatypes::Metadata,
    idx_field: Field,
    idx_col: ArrayRef,
    val_field: Field,
    val_col: ArrayRef,
) -> Result<PathBuf> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let schema =
        arrow::datatypes::Schema::new_with_metadata(vec![idx_field, val_field], meta.clone());
    let batch = RecordBatch::try_new(Arc::new(schema), vec![idx_col, val_col])?;
    let path = dir.join(format!("{stem}_{kind}.parquet"));
    write_atomic(&path, |tmp| {
        let file = File::create(tmp)?;
        let mut writer = ArrowWriter::try_new(
            file,
            batch.schema(),
            Some(WriterProperties::builder().build()),
        )?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    })?;
    Ok(path)
}
