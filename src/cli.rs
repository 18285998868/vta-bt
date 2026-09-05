use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "vector-ta", about = "向量化技术指标回测框架")]
pub struct Cli {
    #[arg(short, long, default_value = "config/config.toml")]
    pub config: String,
}

pub fn parse() -> Cli {
    Cli::parse()
}
