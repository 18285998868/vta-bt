use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "vta", version, about = "向量化技术指标回测框架")]
/// 命令行参数。`-c` 跑回测, `--pbo` 实算 PBO, `--analyze` 只读分析报告;
/// 三者必须且只能给一个。
pub struct Cli {
    /// 回测模式: 配置文件路径。
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// PBO 模式: 读取该报告目录 `detail/manifest.toml` 索引列出的
    /// `*_fixedcurve.parquet`, 按(币,周期,策略)分组实算 CSCV。只使用
    /// `status == ok` 且共享同一时间网格的曲线。与 `-c` / `--analyze` 三选一。
    #[arg(long)]
    pub pbo: Option<PathBuf>,

    /// 分析模式: 只读该报告目录(需含 `result.parquet`, 可选 `detail/`),
    /// 输出对账、描述性统计、回撤、交易结构、PSR/DSR/MinTRL 与平稳 bootstrap。
    /// 不重跑回测, 不修改报告产物。
    #[arg(long, value_name = "DIR", conflicts_with_all = ["config", "pbo"])]
    pub analyze: Option<PathBuf>,

    /// 分析产物输出目录; 默认 `<analyze>/analysis`。
    #[arg(long, value_name = "DIR", requires = "analyze")]
    pub out: Option<PathBuf>,

    /// 分析: Markdown 中 Top-N 行数。
    #[arg(long, value_name = "N", default_value_t = 20, requires = "analyze")]
    pub top: usize,

    /// 分析: 平稳 bootstrap 重采样次数; 0 表示跳过。
    #[arg(long, value_name = "N", default_value_t = 2_000, requires = "analyze")]
    pub bootstrap: usize,

    /// 分析: 平稳 bootstrap 块长(bar 数); 省略时用 Politis-White 自动选择。
    #[arg(long, value_name = "BARS", requires = "analyze")]
    pub block_len: Option<usize>,

    /// 分析: 随机种子; 同一输入 + 同一 seed 输出逐位一致。
    #[arg(long, value_name = "U64", default_value_t = 42, requires = "analyze")]
    pub seed: u64,

    /// 分析: 显著性水平(MinTRL 与 bootstrap 置信区间)。
    #[arg(long, value_name = "P", default_value_t = 0.05, requires = "analyze")]
    pub alpha: f64,

    /// 分析: 参与排名/显著性的最少成交笔数。
    #[arg(long, value_name = "N", default_value_t = 20, requires = "analyze")]
    pub min_trades: u32,

    /// 分析: DSR 的试验数覆盖; 省略时取同组同窗口 `status == ok` 的行数。
    #[arg(long, value_name = "N", requires = "analyze")]
    pub trials: Option<usize>,

    /// 分析: detail 曲线数上限, 超过则拒绝分析(避免大网格 OOM)。
    #[arg(long, value_name = "N", default_value_t = 2_000, requires = "analyze")]
    pub max_curves: usize,

    /// 分析: 要求 `detail/` 必须存在; 缺失时直接报错。
    #[arg(long, requires = "analyze")]
    pub require_detail: bool,

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
