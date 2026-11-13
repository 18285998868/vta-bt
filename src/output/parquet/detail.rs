//! 逐笔明细与权益曲线的落盘、清单与缓存校验。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field};
use arrow::record_batch::RecordBatch;

use crate::error::Result;

use super::columns::*;
use super::report::{
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

        // 复利腿逐 bar 权益。README 承诺 `detail/` 导出"逐笔交易/权益曲线",
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

/// 按**列名**取列序号; 缺列时返回错误并列出实际列名。
///
/// 优于魔法列号: 曲线文件的列顺序由 schema 决定, 用常量下标读取会在 schema
/// 调整后**静默读错列**(类型恰好兼容时不报错, 数值静默错位)。
///
/// # Errors
/// schema 中不存在 `name` 列时返回 `AppError`(crate::error::AppError), 消息里列出实际存在的列名。
pub fn col_by_name(
    schema: &arrow::datatypes::Schema,
    name: &str,
    file: &std::path::Path,
) -> Result<usize> {
    schema.index_of(name).map_err(|_| {
        crate::error::AppError::Msg(format!(
            "{} 缺列 {name:?}; 实际列: {:?}",
            file.display(),
            schema
                .fields()
                .iter()
                .map(|f| f.name().as_str())
                .collect::<Vec<_>>()
        ))
    })
}

/// 权益曲线文件元组。
#[derive(Debug, Clone)]
/// 权益曲线文件元组。
pub struct EquityCurve {
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,

    /// bar 时间戳(毫秒)。
    pub times: Vec<i64>,
    /// 复利腿逐 bar 权益。
    pub values: Vec<f64>,
}

/// 读取目录下全部 `*_equitycurve.parquet`(一般为 detail/ 目录)。
///
/// 与 [`read_fixed_curves`] 同构但**列语义不同**: 前者返回 `(time, equity)`,
/// 后者返回 `(bar_idx, fixed_equity)`。刻意不复用同一个 reader —— 两条曲线的
/// 索引列类型(Int64 vs UInt64)、含义(时间戳 vs 序号)都不同, 混用会静默误读。
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列、缺分组元数据, 或列类型不符时
/// 返回 `AppError`(crate::error::AppError)。
pub fn read_equity_curves(dir: &Path) -> Result<Vec<EquityCurve>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs;
    use std::fs::File;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        crate::error::AppError::Msg(format!("读取 equitycurve 目录失败 {}: {e}", dir.display()))
    })?;
    // 目录项的 IO 错误**不得**被 `filter_map(|e| e.ok())` 吞掉: 那会让
    // "只读到一半曲线"被当成"成功返回", 而 `--pbo` 会基于**不同的策略集合**
    // 算出 φ, 无任何提示。逐项显式传播错误。
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            crate::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet")
            && p.file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_equitycurve"))
        {
            paths.push(p);
        }
    }
    paths.sort();
    for path in paths {
        let file = File::open(&path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let schema = builder.schema().clone();
        let idx = col_by_name(&schema, "time", &path)?;
        let val = col_by_name(&schema, "equity", &path)?;
        let meta = schema.metadata().clone();
        let get = |k: &str| -> Result<String> {
            meta.get(k).cloned().ok_or_else(|| {
                crate::error::AppError::Msg(format!(
                    "equitycurve 缺分组元数据 {k}: {}",
                    path.display()
                ))
            })
        };
        let symbol = get("symbol")?;
        let timeframe = get("timeframe")?;
        let strategy = get("strategy")?;
        let params = get("params")?;
        let reader = builder.build()?;
        let mut times = Vec::new();
        let mut values = Vec::new();
        for batch in reader {
            let batch = batch?;
            times.extend(i64_col(&batch, idx, &path)?);
            values.extend(f64_col(&batch, val, &path)?);
        }
        out.push(EquityCurve {
            symbol,
            timeframe,
            strategy,
            params,
            times,
            values,
        });
    }
    Ok(out)
}

/// fixed 曲线文件元组(供 `--pbo` 按 (币, 周期, 策略) 分组)。
#[derive(Debug, Clone)]
pub struct FixedCurve {
    /// 产出该曲线的组合缓存 id(`temps/` 文件名 stem)。
    ///
    /// detail 文件名只含参数串, 不同数据窗口/源码版本会用**同一个文件名**互相
    /// 覆盖。`combo` 记录"这个文件是哪一轮写的", 缓存命中校验与 `--pbo` 索引
    /// 用它发现"索引说 A 轮, 文件其实是 B 轮"的静默错配。
    pub combo: String,
    /// 品种。
    pub symbol: String,
    /// 周期。
    pub timeframe: String,
    /// 策略名。
    pub strategy: String,
    /// 参数串原文。
    pub params: String,
    /// 该档位的运行状态。
    pub status: crate::app::RunStatus,
    /// bar 时间戳(毫秒), 与 [`Self::values`] 等长。
    ///
    /// 取代旧的 `bar_idx`: PBO 只应比较**同一时间网格**上的曲线。仅凭下标
    /// 无法发现两条曲线来自不同数据窗口, 只能按 `min(len)` 静默截断。
    pub times: Vec<i64>,
    /// fixed 腿逐 bar 权益。
    pub values: Vec<f64>,
}

/// detail 目录索引文件名(位于 `detail/manifest.toml`)。
pub const DETAIL_INDEX_FILE: &str = "manifest.toml";
// v2: 索引条目增加 `size` 字段, 读取时校验文件未被截断/替换。
pub(super) const DETAIL_MANIFEST_VERSION: u32 = 2;

/// 单个组合的 detail 侧车清单: 记录该组合写出的全部文件及大小。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct ComboDetailManifest {
    version: u32,
    combo: String,
    files: Vec<DetailArtifact>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct DetailArtifact {
    name: String,
    bytes: u64,
}

/// `detail/manifest.toml`: 只列**当前一轮**产出且仍有效的 fixed 曲线。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct DetailIndex {
    version: u32,
    entries: Vec<DetailIndexEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct DetailIndexEntry {
    name: String,
    combo: String,
    /// 写入索引时的字节数; 读取时校验, 防止文件被截断/替换后仍参与 PBO。
    size: u64,
}

pub(super) fn combo_id(combo_path: &Path) -> String {
    combo_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn combo_manifest_path(combo_path: &Path) -> PathBuf {
    combo_path.with_extension("detail.toml")
}

/// 只接受"单个普通文件名", 防止索引中的 `../` 把读取带出 detail 目录。
pub(super) fn is_plain_filename(name: &str) -> bool {
    let mut comps = Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

/// 写入单个组合的 detail 侧车清单。
///
/// 文件已由 [`write_detail_parquet`] 落盘; 这里记录**文件名 + 字节数**, 供
/// 下一次缓存命中时校验"整组产物是否仍在、是否被截断"。清单本身不是缓存
/// 指纹的一部分 —— 缺失/损坏只会触发一次重算, 不会让错误结果继续传播。
///
/// # Errors
/// 任一产物不存在, 或清单写入失败时返回 `AppError`(crate::error::AppError)。
pub fn write_combo_detail_manifest(
    combo_path: &Path,
    detail_dir: &Path,
    files: &[PathBuf],
) -> Result<()> {
    let combo = combo_id(combo_path);
    let mut artifacts = Vec::with_capacity(files.len());
    for file in files {
        if file.parent() != Some(detail_dir) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 产物不在 detail 目录 {}: {}",
                detail_dir.display(),
                file.display()
            )));
        }
        let meta = std::fs::metadata(file).map_err(|e| {
            crate::error::AppError::Msg(format!("detail 产物写后校验失败 {}: {e}", file.display()))
        })?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("detail 产物没有文件名: {}", file.display()))
            })?;
        if !is_plain_filename(&name) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 产物文件名非法: {name:?}"
            )));
        }
        artifacts.push(DetailArtifact {
            name,
            bytes: meta.len(),
        });
    }
    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    artifacts.dedup_by(|a, b| a.name == b.name);
    let manifest = ComboDetailManifest {
        version: DETAIL_MANIFEST_VERSION,
        combo,
        files: artifacts,
    };
    let text = toml::to_string(&manifest)
        .map_err(|e| crate::error::AppError::Msg(format!("序列化 detail 清单失败: {e}")))?;
    let path = combo_manifest_path(combo_path);
    write_atomic(&path, |tmp| {
        std::fs::write(tmp, text.as_bytes())?;
        Ok(())
    })
}

/// 重算组合前删除旧侧车清单。
///
/// 必须**在写任何 detail 文件之前**调用: 重算会覆盖同名 detail 文件, 但组合
/// parquet 要到最后才写。若中途崩溃而旧清单仍在, 下一轮会把"新 detail + 旧
/// result"误判为完整缓存(文件名、大小、`combo` 都可能相同), 静默产出错配数据。
/// 先删清单 ⇒ 崩溃后下一轮必然重算。
///
/// # Errors
/// 删除失败(非"文件不存在")时返回 `AppError`(crate::error::AppError)。
pub fn invalidate_combo_detail_manifest(combo_path: &Path) -> Result<()> {
    let path = combo_manifest_path(combo_path);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn read_combo_detail_manifest(combo_path: &Path) -> Result<ComboDetailManifest> {
    let path = combo_manifest_path(combo_path);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "缺少 detail 侧车清单 {}: {e}; 该组合的 detail 产物不完整, 请重跑",
            path.display()
        ))
    })?;
    let manifest: ComboDetailManifest = toml::from_str(&text)?;
    if manifest.version != DETAIL_MANIFEST_VERSION {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 不支持的 detail 清单版本 {}",
            path.display(),
            manifest.version
        )));
    }
    if manifest.combo != combo_id(combo_path) {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 清单 combo={} 与组合文件名不符(文件可能被复制/改名)",
            path.display(),
            manifest.combo
        )));
    }
    Ok(manifest)
}

/// 校验组合缓存对应的 detail 产物是否完整且属于该组合。
///
/// 返回 `Ok(false)` 表"需要重算"(已 `warn!` 说明原因); 只有无法判断的 IO
/// 错误才上抛。校验项:
/// 1. 侧车清单存在、版本/组合 id 匹配;
/// 2. 清单中每个文件存在且字节数未变(截断/半写会被发现);
/// 3. 文件 metadata 的 `combo` 与清单一致(同名文件被另一轮数据窗口覆盖会被发现)。
///
/// # Errors
/// 清单读取遇到非文件不存在的 IO 错误时返回 `AppError`(crate::error::AppError)。
pub fn detail_cache_is_complete(combo_path: &Path, detail_dir: &Path) -> Result<bool> {
    let combo = combo_id(combo_path);
    let manifest_path = combo_manifest_path(combo_path);
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let manifest: ComboDetailManifest = match toml::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "解析 detail 清单失败 {}: {e}; 重算",
                manifest_path.display()
            );
            return Ok(false);
        }
    };
    if manifest.version != DETAIL_MANIFEST_VERSION || manifest.combo != combo {
        tracing::warn!(
            "detail 清单版本/组合 id 不符 {}; 重算",
            manifest_path.display()
        );
        return Ok(false);
    }
    for art in &manifest.files {
        if !is_plain_filename(&art.name) {
            tracing::warn!("detail 清单含非法文件名 {:?}; 重算", art.name);
            return Ok(false);
        }
        let path = detail_dir.join(&art.name);
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("detail 产物缺失 {}: {e}; 重算", path.display());
                return Ok(false);
            }
        };
        if meta.len() != art.bytes {
            tracing::warn!(
                "detail 产物大小变化 {} (清单 {} B, 实际 {} B); 重算",
                path.display(),
                art.bytes,
                meta.len()
            );
            return Ok(false);
        }
        match curve_combo(&path) {
            Ok(c) if c == combo => {}
            Ok(c) => {
                tracing::warn!(
                    "detail 产物属于另一轮组合 {} (期望 {}); 重算: {}",
                    c,
                    combo,
                    path.display()
                );
                return Ok(false);
            }
            Err(e) => {
                tracing::warn!("detail 产物不可读 {}: {e}; 重算", path.display());
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn curve_combo(path: &Path) -> Result<String> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    builder
        .schema()
        .metadata()
        .get("combo")
        .cloned()
        .ok_or_else(|| {
            crate::error::AppError::Msg(format!(
                "{}: 缺 metadata combo; 该文件可能由旧版本写出, 请删掉后重跑",
                path.display()
            ))
        })
}

/// 汇总本轮全部组合的侧车清单, 写 `detail/manifest.toml`。
///
/// 索引**只列 fixed 曲线**: `--pbo` 只消费它们。目录里上一轮遗留、或参数已从
/// 网格中删除的曲线不会进索引, 因而不会静默参与 CSCV。
///
/// # Errors
/// 任一组合缺侧车清单, 或索引写入失败时返回 `AppError`(crate::error::AppError)。
pub fn write_detail_index(detail_dir: &Path, combo_paths: &[PathBuf]) -> Result<()> {
    let mut entries = Vec::new();
    for combo_path in combo_paths {
        let manifest = read_combo_detail_manifest(combo_path)?;
        for file in &manifest.files {
            if file.name.ends_with("_fixedcurve.parquet") {
                entries.push(DetailIndexEntry {
                    name: file.name.clone(),
                    combo: manifest.combo.clone(),
                    size: file.bytes,
                });
            }
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.combo.cmp(&b.combo)));
    entries.dedup_by(|a, b| a.name == b.name && a.combo == b.combo);
    let index = DetailIndex {
        version: DETAIL_MANIFEST_VERSION,
        entries,
    };
    let text = toml::to_string(&index)
        .map_err(|e| crate::error::AppError::Msg(format!("序列化 detail 索引失败: {e}")))?;
    let path = detail_dir.join(DETAIL_INDEX_FILE);
    write_atomic(&path, |tmp| {
        std::fs::write(tmp, text.as_bytes())?;
        Ok(())
    })
}

/// 读取目录下全部 `*_fixedcurve.parquet`(一般为 detail/ 目录)。
///
/// 这是**目录扫描**版本, 会把上一轮遗留的多余文件也读进来。`--pbo` 走
/// [`read_fixed_curves_from_manifest`], 只读当前索引列出的文件; 本函数保留给
/// 需要查看目录全貌的调用方/测试。
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列、缺分组元数据, 或列类型不符时
/// 返回 `AppError`(crate::error::AppError)。
pub fn read_fixed_curves(dir: &Path) -> Result<Vec<FixedCurve>> {
    use std::fs;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        crate::error::AppError::Msg(format!("读取 fixedcurve 目录失败 {}: {e}", dir.display()))
    })?;
    // 同 `read_equity_curves`: 目录项的 IO 错误必须显式传播, 不得被吞掉
    // (否则 `--pbo` 会基于少读一半曲线的集合算 φ, 且无任何提示)。
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            crate::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet")
            && p.file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_fixedcurve"))
        {
            paths.push(p);
        }
    }
    paths.sort();
    for path in paths {
        out.push(read_fixed_curve(&path)?);
    }
    Ok(out)
}

/// 读取 `detail/manifest.toml` 列出的 fixed 曲线(不扫描目录)。
///
/// # Errors
/// 索引缺失/解析失败、索引列出的文件缺失、文件内 `combo` 与索引不一致,
/// 或曲线 schema/metadata 不合法时返回 `AppError`(crate::error::AppError)。
pub fn read_fixed_curves_from_manifest(dir: &Path) -> Result<Vec<FixedCurve>> {
    let path = dir.join(DETAIL_INDEX_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::AppError::Msg(format!(
            "缺少 detail 索引 {}: {e}; 请先用 detail=true 且 fixed_curves=true 重跑",
            path.display()
        ))
    })?;
    let index: DetailIndex = toml::from_str(&text)?;
    if index.version != DETAIL_MANIFEST_VERSION {
        return Err(crate::error::AppError::Msg(format!(
            "{}: 不支持的 detail 索引版本 {}",
            path.display(),
            index.version
        )));
    }
    let mut out = Vec::with_capacity(index.entries.len());
    for entry in index.entries {
        if !is_plain_filename(&entry.name) {
            return Err(crate::error::AppError::Msg(format!(
                "detail 索引含非法文件名 {:?}",
                entry.name
            )));
        }
        let file = dir.join(&entry.name);
        let actual = std::fs::metadata(&file)?.len();
        if actual != entry.size {
            return Err(crate::error::AppError::Msg(format!(
                "{}: 索引记录 {} B, 实际 {} B; 文件被截断/替换或目录未同步, 请重跑",
                file.display(),
                entry.size,
                actual
            )));
        }
        let curve = read_fixed_curve(&file)?;
        if curve.combo != entry.combo {
            return Err(crate::error::AppError::Msg(format!(
                "{}: 索引记录 combo={}, 文件内 combo={}; detail 目录可能被另一轮运行覆盖, 请重跑",
                file.display(),
                entry.combo,
                curve.combo
            )));
        }
        out.push(curve);
    }
    Ok(out)
}

pub(super) fn read_fixed_curve(path: &Path) -> Result<FixedCurve> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let time_idx = col_by_name(&schema, "time", path)?;
    // 按列名取值列, 而非魔法下标 1: 理由同 `read_equity_curves` —— 下标会在
    // schema 调整后静默读到类型恰好兼容的另一列(不报错, 数值错位)。
    let val_idx = col_by_name(&schema, "fixed_equity", path)?;
    let meta = schema.metadata().clone();
    let get = |k: &str| -> Result<String> {
        meta.get(k).cloned().ok_or_else(|| {
            crate::error::AppError::Msg(format!("fixedcurve 缺分组元数据 {k}: {}", path.display()))
        })
    };
    let combo = get("combo")?;
    let symbol = get("symbol")?;
    let timeframe = get("timeframe")?;
    let strategy = get("strategy")?;
    let params = get("params")?;
    let status_text = get("status")?;
    let status = crate::app::RunStatus::parse(&status_text).ok_or_else(|| {
        crate::error::AppError::Msg(format!(
            "{}: 未知 status {status_text:?}; 文件可能由其它版本产出, 请删掉后重跑",
            path.display()
        ))
    })?;
    let reader = builder.build()?;
    let mut times = Vec::new();
    let mut values = Vec::new();
    for batch in reader {
        let batch = batch?;
        times.extend(i64_col(&batch, time_idx, path)?);
        values.extend(f64_col(&batch, val_idx, path)?);
    }
    if times.len() != values.len() {
        return Err(crate::error::AppError::Msg(format!(
            "{}: time 列 {} 行 != fixed_equity 列 {} 行; 文件被截断或损坏",
            path.display(),
            times.len(),
            values.len()
        )));
    }
    Ok(FixedCurve {
        combo,
        symbol,
        timeframe,
        strategy,
        params,
        status,
        times,
        values,
    })
}
