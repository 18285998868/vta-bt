//! 输出配置与路径安全检查。

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;

/// 输出配置。`dir` 为输出根目录，合并结果写入 `{dir}/result.parquet`，逐组合写入 `{dir}/temps/`。
/// 若未设置，默认使用当前目录 `./reports/`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// 输出根目录; 缺省 `./reports/`。
    pub dir: Option<PathBuf>,
    /// 是否导出逐笔交易和权益曲线详细数据
    #[serde(default)]
    pub detail: bool,
    /// 是否写 fixedcurve 文件（`--pbo` 的 T×N 输入）。默认 false：
    /// detail 仍会写逐笔 trades 与权益曲线，只是不写 fixed 曲线；
    /// 做 PBO 时显式开 true（且必须同时 `detail = true`）。
    #[serde(default)]
    pub fixed_curves: bool,
    /// 是否覆盖已存在的回测结果文件，默认 false（跳过已完成的组合）
    #[serde(default)]
    pub overwrite: bool,
}

/// 校验一个会**进入落盘文件名**的字符串是安全的路径片段。
///
/// 拒绝:
/// - 任何路径分隔符(`/`、`\`)、盘符分隔符(`:`);
/// - `..`(父目录)与 `.`(当前目录)段;
/// - 其它控制字符与空白(它们在文件名里毫无意义, 还会让跨平台行为不一致);
/// - 空串。
///
/// 允许: 字母、数字、`.`、`-`、`_`、`+` —— 覆盖全部真实品种名
/// (`BTCUSDT` / `1000SHIBUSDT` / `1INCHUSDT` / `BRK-B`) 与周期名(`1m` / `1d`)。
///
/// # Errors
/// 不安全时返回 `AppError`。
pub(super) fn check_path_safe_component(what: &str, value: &str) -> crate::error::Result<()> {
    if value.is_empty() {
        return Err(AppError::Msg(format!("{what} 不能为空")));
    }
    if value == "." || value == ".." {
        return Err(AppError::Msg(format!(
            "{what} = {value:?} 是路径片段, 不得直接进入文件名"
        )));
    }
    for c in value.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+');
        if !ok {
            return Err(AppError::Msg(format!(
                "{what} = {value:?} 含非法字符 {c:?}; \
                 它会直接进入落盘文件名, 只允许字母数字与 `.` `-` `_` `+`"
            )));
        }
    }
    Ok(())
}
