//! 回测编排层: 加载配置、构建策略、逐组合执行并落盘。

mod combo;
mod fingerprint;
mod run;
mod types;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use fingerprint::source_hash;
pub use run::{RunOptions, run, run_with};
pub use types::{DetailResult, RunResult, RunStatus};
