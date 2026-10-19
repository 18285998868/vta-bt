use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "vector-ta", about = "向量化技术指标回测框架")]
/// 命令行参数。`-c` 跑回测, `--pbo` 实算 PBO, 二者必须且只能给一个。
pub struct Cli {
    /// 回测模式: 配置文件路径。
    #[arg(short, long)]
    pub config: Option<String>,

    /// PBO 模式: 读取该报告目录 detail/ 下 `*_fixedcurve.parquet`,
    /// 按(币,周期,策略)分组实算 CSCV。与 `-c` 二选一。
    #[arg(long)]
    pub pbo: Option<String>,
}

/// 解析命令行参数。
///
/// # Panics
/// 参数非法时 `clap` 会打印用法并退出。
#[must_use]
pub fn parse() -> Cli {
    Cli::parse()
}
