use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "vta", version, about = "向量化技术指标回测框架")]
/// 命令行参数。`-c` 跑回测, `--pbo` 实算 PBO, 二者必须且只能给一个。
pub struct Cli {
    /// 回测模式: 配置文件路径。
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// PBO 模式: 读取该报告目录 `detail/manifest.toml` 索引列出的
    /// `*_fixedcurve.parquet`, 按(币,周期,策略)分组实算 CSCV。只使用
    /// `status == ok` 且共享同一时间网格的曲线。与 `-c` 二选一。
    #[arg(long)]
    pub pbo: Option<PathBuf>,

    /// **忽略全部缓存**, 强制重算(逃生门)。
    ///
    /// 正常情况下缓存已足够安全: 指纹覆盖行情内容、资金、窗口、执行口径、
    /// 产出开关、**实现版本**(源码内容哈希) ⇒ 代码/数据/配置任一变化都会
    /// 自动失效。本开关用于"明知缓存可能过期、但只想强制重算一次"的场合
    /// (例如怀疑环境有非确定性差异时做对照跑)。
    #[arg(long)]
    pub no_cache: bool,
}

/// 解析命令行参数。
///
/// # Panics
/// 参数非法时 `clap` 会打印用法并退出。
#[must_use]
pub fn parse() -> Cli {
    Cli::parse()
}
