use std::path::Path;

use crate::error::Result;

use super::super::columns::{f64_col, i64_col};
use super::manifest::{DETAIL_INDEX_FILE, DETAIL_MANIFEST_VERSION, DetailIndex, is_plain_filename};

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
