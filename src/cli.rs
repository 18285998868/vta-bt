use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "vector-ta", about = "向量化技术指标回测框架")]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<String>,

    /// PBO 模式: 读取该报告目录 detail/ 下 `*_fixedcurve.parquet`,
    /// 按(币,周期,策略)分组实算 CSCV。与 `-c` 二选一。
    #[arg(long)]
    pub pbo: Option<String>,
}

pub fn parse() -> Cli {
    Cli::parse()
}
