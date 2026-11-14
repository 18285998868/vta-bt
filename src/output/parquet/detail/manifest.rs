use std::path::{Path, PathBuf};

use crate::error::Result;

use super::super::report::write_atomic;

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
    pub(super) version: u32,
    pub(super) entries: Vec<DetailIndexEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct DetailIndexEntry {
    pub(super) name: String,
    pub(super) combo: String,
    /// 写入索引时的字节数; 读取时校验, 防止文件被截断/替换后仍参与 PBO。
    pub(super) size: u64,
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
/// 文件已由 [`super::write_detail_parquet`] 落盘; 这里记录**文件名 + 字节数**, 供
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
