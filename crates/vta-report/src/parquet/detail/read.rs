use std::collections::HashSet;
use std::path::Path;

use vta_core::RunStatus;
use vta_core::engine::Side;
use vta_core::error::Result;
use vta_core::money::Amount;

use super::super::columns::{dec_col, f64_col, i64_col, str_col};
use super::manifest::{
    DETAIL_INDEX_FILE, DETAIL_MANIFEST_VERSION, DetailIndex, is_plain_filename,
    read_combo_detail_manifest,
};

/// 按**列名**取列序号; 缺列时返回错误并列出实际列名。
///
/// 优于魔法列号: 曲线文件的列顺序由 schema 决定, 用常量下标读取会在 schema
/// 调整后**静默读错列**(类型恰好兼容时不报错, 数值静默错位)。
///
/// # Errors
/// schema 中不存在 `name` 列时返回 `AppError`(vta_core::error::AppError), 消息里列出实际存在的列名。
pub fn col_by_name(
    schema: &arrow::datatypes::Schema,
    name: &str,
    file: &std::path::Path,
) -> Result<usize> {
    schema.index_of(name).map_err(|_| {
        vta_core::error::AppError::Msg(format!(
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

/// detail 文件 schema metadata 里的分组信息。
#[derive(Debug, Clone)]
struct GroupMeta {
    combo: String,
    symbol: String,
    timeframe: String,
    strategy: String,
    params: String,
    status: RunStatus,
}

fn group_meta(schema: &arrow::datatypes::Schema, kind: &str, file: &Path) -> Result<GroupMeta> {
    let meta = schema.metadata();
    let get = |k: &str| -> Result<String> {
        meta.get(k).cloned().ok_or_else(|| {
            vta_core::error::AppError::Msg(format!("{kind} 缺分组元数据 {k}: {}", file.display()))
        })
    };
    let status_text = get("status")?;
    let status = RunStatus::parse(&status_text).ok_or_else(|| {
        vta_core::error::AppError::Msg(format!(
            "{}: 未知 status {status_text:?}; 文件可能由其它版本产出, 请删掉后重跑",
            file.display()
        ))
    })?;
    Ok(GroupMeta {
        combo: get("combo")?,
        symbol: get("symbol")?,
        timeframe: get("timeframe")?,
        strategy: get("strategy")?,
        params: get("params")?,
        status,
    })
}

/// 权益曲线文件元组。
#[derive(Debug, Clone)]
pub struct EquityCurve {
    /// 产出该曲线的组合缓存 id(`temps/` 文件名 stem), 用于与 fixed 曲线/索引配对。
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
    pub status: RunStatus,

    /// bar 时间戳(毫秒)。
    pub times: Vec<i64>,
    /// 复利腿逐 bar 权益。
    pub values: Vec<f64>,
}

/// 读取目录下全部 `*_equitycurve.parquet`(一般为 detail/ 目录)。
///
/// 与 [`read_fixed_curves`] 同构但**列语义不同**: 前者返回 `(time, equity)`,
/// 后者返回 `(time, fixed_equity)`。刻意不复用同一个 reader —— 两条曲线的
/// 列含义不同, 混用会静默误读。
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列、缺分组元数据, 或列类型不符时
/// 返回 `AppError`(vta_core::error::AppError)。
pub fn read_equity_curves(dir: &Path) -> Result<Vec<EquityCurve>> {
    use std::fs;

    let mut paths = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        vta_core::error::AppError::Msg(format!("读取 equitycurve 目录失败 {}: {e}", dir.display()))
    })?;
    for e in entries {
        let e = e.map_err(|err| {
            vta_core::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if is_curve_file(&p, "equitycurve") {
            paths.push(p);
        }
    }
    paths.sort();
    paths.iter().map(|p| read_equity_curve(p)).collect()
}

/// 读取 `detail/manifest.toml` 列出的组合对应的 `*_equitycurve.parquet`。
///
/// 索引只记录 fixed 曲线的文件名与大小; 本函数按同一 stem 推导复利曲线名,
/// 并校验文件内 `combo` 与索引一致。缺文件直接报错: 部分加载会让后续分析
/// 基于不完整的组合集合, 且无任何提示。
///
/// # Errors
/// 索引缺失/解析失败、索引列出的 fixed 曲线无对应 equity 文件, 或文件
/// schema/metadata 不合法时返回 `AppError`。
pub fn read_equity_curves_from_manifest(dir: &Path) -> Result<Vec<EquityCurve>> {
    let index = read_detail_index(dir)?;
    let mut out = Vec::with_capacity(index.entries.len());
    for entry in index.entries {
        let stem = fixed_stem(&entry.name)?;
        let file = dir.join(format!("{stem}_equitycurve.parquet"));
        let curve = read_equity_curve(&file)?;
        if curve.combo != entry.combo {
            return Err(vta_core::error::AppError::Msg(format!(
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

/// 逐笔成交记录的读取视图。
///
/// 不复用 [`vta_core::engine::Trade`]: 后者的 `exit_reason` 是 `&'static str`
/// (封闭枚举的展示文本), 而磁盘上可能是旧版本写出的其它字符串; 读取层保留
/// 原文, 让分析层显式统计未知值, 而不是在读取时静默映射。
#[derive(Debug, Clone)]
pub struct TradeRecord {
    /// 开仓 bar 时间戳(ms)。
    pub entry_time: i64,
    /// 平仓 bar 时间戳(ms)。
    pub exit_time: i64,
    /// 持仓方向。
    pub side: Side,
    /// 开仓成交价。
    pub entry_price: f64,
    /// 平仓成交价。
    pub exit_price: f64,
    /// 复利腿持仓数量。
    pub qty: Amount,
    /// 复利腿盈亏。
    pub pnl: Amount,
    /// 相对开仓价的收益率(小数)。
    pub pnl_pct: f64,
    /// fixed 腿盈亏。
    pub fixed_pnl: Amount,
    /// 持仓 bar 数。
    pub bars_held: i64,
    /// 持仓期最大有利偏移(价格差)。
    pub max_runup: f64,
    /// 持仓期最大不利偏移(价格差)。
    pub max_drawdown: f64,
    /// 平仓原因原文。
    pub exit_reason: String,
}

/// 一组逐笔成交(单个组合)。
#[derive(Debug, Clone)]
pub struct TradeSet {
    /// 组合缓存 id。
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
    pub status: RunStatus,
    /// 逐笔成交。
    pub trades: Vec<TradeRecord>,
}

/// 读取目录下全部逐笔成交文件(一般为 detail/ 目录)。
///
/// 排除 `*_equitycurve.parquet` 与 `*_fixedcurve.parquet`: 它们与成交文件
/// 同目录、同后缀, 只能靠文件名区分。
///
/// # Errors
/// 目录读取失败、文件打开/解析失败、缺列或 metadata 不合法时返回
/// `AppError`(vta_core::error::AppError)。
pub fn read_trades(dir: &Path) -> Result<Vec<TradeSet>> {
    use std::fs;

    let entries = fs::read_dir(dir).map_err(|e| {
        vta_core::error::AppError::Msg(format!("读取 trades 目录失败 {}: {e}", dir.display()))
    })?;
    let mut paths = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            vta_core::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet")
            && !p
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_equitycurve"))
            && !p
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().ends_with("_fixedcurve"))
        {
            paths.push(p);
        }
    }
    paths.sort();
    paths.iter().map(|p| read_trade_set(p)).collect()
}

/// 一个 detail 组合的完整产物: fixed 曲线、复利曲线与逐笔成交。
#[derive(Debug, Clone)]
pub struct DetailBundle {
    /// 由 `detail/manifest.toml` 索引列出的 fixed 曲线。
    pub fixed: Vec<FixedCurve>,
    /// 与 fixed 曲线同 stem 的复利曲线; 文件缺失时不会出现在这里。
    pub equity: Vec<EquityCurve>,
    /// 与 fixed 曲线同 stem 的逐笔成交; 文件缺失时不会出现在这里。
    pub trades: Vec<TradeSet>,
    /// 非致命问题(缺侧车清单、缺可选文件等), 由调用方写入分析报告。
    pub warnings: Vec<String>,
}

/// 读取报告目录的 detail 产物, 并做完整性校验。
///
/// `report_dir` 可以是报告根目录(内含 `detail/` 与 `temps/`)或 detail
/// 目录本身。校验顺序:
/// 1. `detail/manifest.toml` 的版本、文件名、fixed 文件字节数与 `combo`;
/// 2. 与 fixed 同 stem 的 equity/trades 若存在, 其 `combo` / `status` 必须一致,
///    equity 的时间轴必须与 fixed 完全相同;
/// 3. 若 `temps/<combo>.detail.toml` 侧车清单存在, 清单内每个文件的字节数
///    必须与磁盘一致(覆盖 equity/trades, 索引只记录 fixed)。
///
/// equity/trades 是可选产物: 只做 fixed 腿分析时允许缺; 缺失会写入
/// [`DetailBundle::warnings`]。缺 fixed 索引则直接报错。
///
/// # Errors
/// 索引缺失/损坏、文件大小或 `combo` 不符、曲线时间轴不一致时返回
/// `AppError`(vta_core::error::AppError)。
pub fn read_detail_bundle(report_dir: &Path) -> Result<DetailBundle> {
    let nested = report_dir.join("detail");
    let detail_dir = if nested.join(DETAIL_INDEX_FILE).is_file() {
        nested
    } else {
        report_dir.to_path_buf()
    };
    let temps_dir = if report_dir.join("temps").is_dir() {
        report_dir.join("temps")
    } else {
        report_dir
            .parent()
            .map_or_else(|| report_dir.join("temps"), |p| p.join("temps"))
    };
    let index = read_detail_index(&detail_dir)?;

    let mut warnings = Vec::new();
    let mut fixed = Vec::with_capacity(index.entries.len());
    let mut equity = Vec::new();
    let mut trades = Vec::new();
    // `combo` 是"一轮运行"的缓存 id, 同一 combo 下有多条参数曲线;
    // 去重只能按文件名, 不能按 combo, 否则会丢掉同一轮的大部分曲线。
    let mut seen = HashSet::new();
    let mut sidecars = HashSet::new();

    for entry in index.entries {
        if !seen.insert(entry.name.clone()) {
            warnings.push(format!(
                "detail 索引中文件 {} 出现多次; 只保留首次出现",
                entry.name
            ));
            continue;
        }
        if !is_plain_filename(&entry.name) {
            return Err(vta_core::error::AppError::Msg(format!(
                "detail 索引含非法文件名 {:?}",
                entry.name
            )));
        }
        let file = detail_dir.join(&entry.name);
        let actual = std::fs::metadata(&file)?.len();
        if actual != entry.size {
            return Err(vta_core::error::AppError::Msg(format!(
                "{}: 索引记录 {} B, 实际 {} B; 文件被截断/替换或目录未同步, 请重跑",
                file.display(),
                entry.size,
                actual
            )));
        }
        let curve = read_fixed_curve(&file)?;
        if curve.combo != entry.combo {
            return Err(vta_core::error::AppError::Msg(format!(
                "{}: 索引记录 combo={}, 文件内 combo={}; detail 目录可能被另一轮运行覆盖, 请重跑",
                file.display(),
                entry.combo,
                curve.combo
            )));
        }
        let stem = fixed_stem(&entry.name)?;
        let equity_file = detail_dir.join(format!("{stem}_equitycurve.parquet"));
        if equity_file.is_file() {
            let eq = read_equity_curve(&equity_file)?;
            if eq.combo != curve.combo {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: combo={} 与 fixed 曲线的 combo={} 不一致; 目录被覆盖, 请重跑",
                    equity_file.display(),
                    eq.combo,
                    curve.combo
                )));
            }
            if eq.status != curve.status {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: status={} 与 fixed 曲线 status={} 不一致",
                    equity_file.display(),
                    eq.status.as_str(),
                    curve.status.as_str()
                )));
            }
            if eq.params != curve.params {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: params={} 与 fixed 曲线 params={} 不一致; 文件被同名覆盖, 请重跑",
                    equity_file.display(),
                    eq.params,
                    curve.params
                )));
            }
            if eq.times != curve.times {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: 时间轴与 fixed 曲线不一致; 两条曲线无法逐 bar 对齐",
                    equity_file.display()
                )));
            }
            equity.push(eq);
        } else {
            warnings.push(format!(
                "组合 {} / {} 缺复利曲线 {}; 复利腿分析跳过",
                curve.combo,
                curve.params,
                equity_file.display()
            ));
        }

        let trades_file = detail_dir.join(format!("{stem}.parquet"));
        if trades_file.is_file() {
            let ts = read_trade_set(&trades_file)?;
            if ts.combo != curve.combo {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: combo={} 与 fixed 曲线的 combo={} 不一致; 目录被覆盖, 请重跑",
                    trades_file.display(),
                    ts.combo,
                    curve.combo
                )));
            }
            if ts.status != curve.status {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: status={} 与 fixed 曲线 status={} 不一致",
                    trades_file.display(),
                    ts.status.as_str(),
                    curve.status.as_str()
                )));
            }
            if ts.params != curve.params {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: params={} 与 fixed 曲线 params={} 不一致; 文件被同名覆盖, 请重跑",
                    trades_file.display(),
                    ts.params,
                    curve.params
                )));
            }
            trades.push(ts);
        } else {
            warnings.push(format!(
                "组合 {} / {} 缺逐笔成交文件 {}; 交易结构分析跳过",
                curve.combo,
                curve.params,
                trades_file.display()
            ));
        }

        if sidecars.insert(curve.combo.clone()) {
            validate_sidecar(&temps_dir, &detail_dir, &curve.combo, &mut warnings)?;
        }
        fixed.push(curve);
    }

    Ok(DetailBundle {
        fixed,
        equity,
        trades,
        warnings,
    })
}

fn is_curve_file(path: &Path, kind: &str) -> bool {
    let suffix = format!("_{kind}");
    path.extension().is_some_and(|x| x == "parquet")
        && path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().ends_with(&suffix))
}

fn fixed_stem(name: &str) -> Result<String> {
    name.strip_suffix("_fixedcurve.parquet")
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            vta_core::error::AppError::Msg(format!(
                "detail 索引文件名 {name:?} 不以 _fixedcurve.parquet 结尾"
            ))
        })
}

fn read_detail_index(dir: &Path) -> Result<DetailIndex> {
    let path = dir.join(DETAIL_INDEX_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        vta_core::error::AppError::Msg(format!(
            "缺少 detail 索引 {}: {e}; 请先用 detail=true 且 fixed_curves=true 重跑",
            path.display()
        ))
    })?;
    let index: DetailIndex = toml::from_str(&text)?;
    if index.version != DETAIL_MANIFEST_VERSION {
        return Err(vta_core::error::AppError::Msg(format!(
            "{}: 不支持的 detail 索引版本 {}",
            path.display(),
            index.version
        )));
    }
    Ok(index)
}

fn validate_sidecar(
    temps_dir: &Path,
    detail_dir: &Path,
    combo: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let combo_path = temps_dir.join(combo);
    let sidecar = combo_path.with_extension("detail.toml");
    if !sidecar.is_file() {
        warnings.push(format!(
            "组合 {combo} 缺侧车清单 {}; 只校验了 fixed 索引, equity/trades 仅按 metadata 配对",
            sidecar.display()
        ));
        return Ok(());
    }
    let manifest = read_combo_detail_manifest(&combo_path)?;
    for art in &manifest.files {
        if !is_plain_filename(&art.name) {
            return Err(vta_core::error::AppError::Msg(format!(
                "{}: 侧车清单含非法文件名 {:?}",
                sidecar.display(),
                art.name
            )));
        }
        let file = detail_dir.join(&art.name);
        let actual = std::fs::metadata(&file)?.len();
        if actual != art.bytes {
            return Err(vta_core::error::AppError::Msg(format!(
                "{}: 侧车清单记录 {} B, 实际 {} B; 文件被截断/替换, 请重跑",
                file.display(),
                art.bytes,
                actual
            )));
        }
    }
    Ok(())
}

/// 读取一条复利曲线文件。
fn read_equity_curve(path: &Path) -> Result<EquityCurve> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let idx = col_by_name(&schema, "time", path)?;
    let val = col_by_name(&schema, "equity", path)?;
    let meta = group_meta(&schema, "equitycurve", path)?;
    let reader = builder.build()?;
    let mut times = Vec::new();
    let mut values = Vec::new();
    for batch in reader {
        let batch = batch?;
        times.extend(i64_col(&batch, idx, path)?);
        values.extend(f64_col(&batch, val, path)?);
    }
    if times.len() != values.len() {
        return Err(vta_core::error::AppError::Msg(format!(
            "{}: time 列 {} 行 != equity 列 {} 行; 文件被截断或损坏",
            path.display(),
            times.len(),
            values.len()
        )));
    }
    Ok(EquityCurve {
        combo: meta.combo,
        symbol: meta.symbol,
        timeframe: meta.timeframe,
        strategy: meta.strategy,
        params: meta.params,
        status: meta.status,
        times,
        values,
    })
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
    pub status: vta_core::RunStatus,
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
/// 返回 `AppError`(vta_core::error::AppError)。
pub fn read_fixed_curves(dir: &Path) -> Result<Vec<FixedCurve>> {
    use std::fs;

    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| {
        vta_core::error::AppError::Msg(format!("读取 fixedcurve 目录失败 {}: {e}", dir.display()))
    })?;
    // 同 `read_equity_curves`: 目录项的 IO 错误必须显式传播, 不得被吞掉
    // (否则 `--pbo` 会基于少读一半曲线的集合算 φ, 且无任何提示)。
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for e in entries {
        let e = e.map_err(|err| {
            vta_core::error::AppError::Msg(format!("读取目录 {} 失败: {err}", dir.display()))
        })?;
        let p = e.path();
        if is_curve_file(&p, "fixedcurve") {
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
/// 或曲线 schema/metadata 不合法时返回 `AppError`(vta_core::error::AppError)。
pub fn read_fixed_curves_from_manifest(dir: &Path) -> Result<Vec<FixedCurve>> {
    let index = read_detail_index(dir)?;
    let mut out = Vec::with_capacity(index.entries.len());
    for entry in index.entries {
        if !is_plain_filename(&entry.name) {
            return Err(vta_core::error::AppError::Msg(format!(
                "detail 索引含非法文件名 {:?}",
                entry.name
            )));
        }
        let file = dir.join(&entry.name);
        let actual = std::fs::metadata(&file)?.len();
        if actual != entry.size {
            return Err(vta_core::error::AppError::Msg(format!(
                "{}: 索引记录 {} B, 实际 {} B; 文件被截断/替换或目录未同步, 请重跑",
                file.display(),
                entry.size,
                actual
            )));
        }
        let curve = read_fixed_curve(&file)?;
        if curve.combo != entry.combo {
            return Err(vta_core::error::AppError::Msg(format!(
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
    let meta = group_meta(&schema, "fixedcurve", path)?;
    let reader = builder.build()?;
    let mut times = Vec::new();
    let mut values = Vec::new();
    for batch in reader {
        let batch = batch?;
        times.extend(i64_col(&batch, time_idx, path)?);
        values.extend(f64_col(&batch, val_idx, path)?);
    }
    if times.len() != values.len() {
        return Err(vta_core::error::AppError::Msg(format!(
            "{}: time 列 {} 行 != fixed_equity 列 {} 行; 文件被截断或损坏",
            path.display(),
            times.len(),
            values.len()
        )));
    }
    Ok(FixedCurve {
        combo: meta.combo,
        symbol: meta.symbol,
        timeframe: meta.timeframe,
        strategy: meta.strategy,
        params: meta.params,
        status: meta.status,
        times,
        values,
    })
}

fn read_trade_set(path: &Path) -> Result<TradeSet> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let idx = |name: &str| col_by_name(&schema, name, path);
    let entry_idx = idx("entry_time")?;
    let exit_idx = idx("exit_time")?;
    let side_idx = idx("side")?;
    let entry_price_idx = idx("entry_price")?;
    let exit_price_idx = idx("exit_price")?;
    let qty_idx = idx("qty")?;
    let pnl_idx = idx("pnl")?;
    let pnl_pct_idx = idx("pnl_pct")?;
    let fixed_pnl_idx = idx("fixed_pnl")?;
    let bars_idx = idx("bars_held")?;
    let runup_idx = idx("max_runup")?;
    let dd_idx = idx("max_drawdown")?;
    let reason_idx = idx("exit_reason")?;
    let meta = group_meta(&schema, "trades", path)?;

    let reader = builder.build()?;
    let mut trades = Vec::new();
    for batch in reader {
        let batch = batch?;
        let entry_time = i64_col(&batch, entry_idx, path)?;
        let exit_time = i64_col(&batch, exit_idx, path)?;
        let side = str_col(&batch, side_idx, path)?;
        let entry_price = f64_col(&batch, entry_price_idx, path)?;
        let exit_price = f64_col(&batch, exit_price_idx, path)?;
        let qty = dec_col(&batch, qty_idx, path)?;
        let pnl = dec_col(&batch, pnl_idx, path)?;
        let pnl_pct = f64_col(&batch, pnl_pct_idx, path)?;
        let fixed_pnl = dec_col(&batch, fixed_pnl_idx, path)?;
        let bars_held = i64_col(&batch, bars_idx, path)?;
        let max_runup = f64_col(&batch, runup_idx, path)?;
        let max_drawdown = f64_col(&batch, dd_idx, path)?;
        let exit_reason = str_col(&batch, reason_idx, path)?;
        let n = entry_time.len();
        for (name, len) in [
            ("exit_time", exit_time.len()),
            ("side", side.len()),
            ("entry_price", entry_price.len()),
            ("exit_price", exit_price.len()),
            ("qty", qty.len()),
            ("pnl", pnl.len()),
            ("pnl_pct", pnl_pct.len()),
            ("fixed_pnl", fixed_pnl.len()),
            ("bars_held", bars_held.len()),
            ("max_runup", max_runup.len()),
            ("max_drawdown", max_drawdown.len()),
            ("exit_reason", exit_reason.len()),
        ] {
            if len != n {
                return Err(vta_core::error::AppError::Msg(format!(
                    "{}: entry_time 列 {n} 行 != {name} 列 {len} 行; 文件被截断或损坏",
                    path.display()
                )));
            }
        }
        for i in 0..n {
            let side = match side[i].as_str() {
                "LONG" => Side::Long,
                "SHORT" => Side::Short,
                other => {
                    return Err(vta_core::error::AppError::Msg(format!(
                        "{}: 第 {i} 笔 side={other:?} 不是 LONG/SHORT",
                        path.display()
                    )));
                }
            };
            trades.push(TradeRecord {
                entry_time: entry_time[i],
                exit_time: exit_time[i],
                side,
                entry_price: entry_price[i],
                exit_price: exit_price[i],
                qty: qty[i],
                pnl: pnl[i],
                pnl_pct: pnl_pct[i],
                fixed_pnl: fixed_pnl[i],
                bars_held: bars_held[i],
                max_runup: max_runup[i],
                max_drawdown: max_drawdown[i],
                exit_reason: exit_reason[i].clone(),
            });
        }
    }
    Ok(TradeSet {
        combo: meta.combo,
        symbol: meta.symbol,
        timeframe: meta.timeframe,
        strategy: meta.strategy,
        params: meta.params,
        status: meta.status,
        trades,
    })
}
