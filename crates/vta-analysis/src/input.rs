//! 报告目录的装载与输入指纹。

use std::path::{Path, PathBuf};

use vta_core::error::{AppError, Result};

use crate::model::ReportBundle;
use crate::stats::fnv1a64;

/// `result.parquet` 的候选路径。报告目录直接包含它; 也接受把组合临时目录
/// 误当报告目录的情况, 但错误信息会明确指向 `result.parquet`。
fn result_path(dir: &Path) -> PathBuf {
    dir.join("result.parquet")
}

fn detail_manifest(dir: &Path) -> Option<PathBuf> {
    let nested = dir.join("detail").join(vta_report::DETAIL_INDEX_FILE);
    if nested.is_file() {
        return Some(nested);
    }
    let flat = dir.join(vta_report::DETAIL_INDEX_FILE);
    flat.is_file().then_some(flat)
}

/// 读取报告目录。`require_detail` 为真时缺 detail 直接报错。
pub(crate) fn load(dir: &Path, require_detail: bool) -> Result<ReportBundle> {
    let result = result_path(dir);
    if !result.is_file() {
        return Err(AppError::Msg(format!(
            "{} 不存在: --analyze 需要包含 result.parquet 的报告目录",
            result.display()
        )));
    }
    let rows = vta_report::read_parquet(&result)?;
    let manifest = detail_manifest(dir);
    if require_detail && manifest.is_none() {
        return Err(AppError::Msg(format!(
            "{} 缺 detail/manifest.toml; 加 --require-detail 时必须有 detail=true 且 fixed_curves=true 的报告",
            dir.display()
        )));
    }
    let detail = match manifest {
        Some(_) => Some(vta_report::read_detail_bundle(dir)?),
        None => None,
    };
    let digest = input_digest(&result, manifest.as_deref(), &rows, detail.as_ref());
    Ok(ReportBundle {
        rows,
        detail,
        input_digest: digest,
    })
}

/// FNV-1a 输入指纹: result.parquet 内容 + detail 索引内容 + fixed 曲线的
/// `(combo, 参数, 长度, 首尾时间)`. 不哈希大曲线内容, 避免为了可追溯性
/// 把 GB 级文件再读一遍; 组合级内容由 `read_detail_bundle` 的大小/combo 校验
/// 保证。
fn input_digest(
    result: &Path,
    manifest: Option<&Path>,
    rows: &[vta_core::RunResult],
    detail: Option<&vta_report::DetailBundle>,
) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = fnv_update_file(h, result);
    if let Some(path) = manifest {
        h = fnv_update_file(h, path);
    }
    for row in rows {
        h ^= u64::from(row.bars_used);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= row.metrics.num_trades as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= fnv1a64(row.symbol.as_bytes());
        h ^= fnv1a64(row.timeframe.as_bytes()).rotate_left(1);
        h ^= fnv1a64(row.strategy.as_bytes()).rotate_left(2);
        h ^= fnv1a64(row.params.as_bytes()).rotate_left(3);
    }
    if let Some(detail) = detail {
        for c in &detail.fixed {
            h ^= fnv1a64(c.combo.as_bytes());
            h ^= fnv1a64(c.params.as_bytes()).rotate_left(1);
            h ^= (c.times.len() as u64).rotate_left(2);
            if let (Some(&first), Some(&last)) = (c.times.first(), c.times.last()) {
                h ^= (first as u64).rotate_left(3);
                h ^= (last as u64).rotate_left(4);
            }
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

fn fnv_update_file(mut h: u64, path: &Path) -> u64 {
    use std::io::Read as _;

    let Ok(mut file) = std::fs::File::open(path) else {
        return h;
    };
    let mut buf = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return h,
            Ok(n) => {
                for &b in &buf[..n] {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            Err(_) => return h,
        }
    }
}
