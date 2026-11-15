//! # vta-core — 领域核心
//!
//! 只含**与 IO 无关**的类型与算法: 错误、定点金额、回测引擎、信号、报告行。
//! 上层 crate 全部单向依赖本 crate, 本 crate 不反向依赖任何 `vta-*`。
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`error`] | 统一错误类型 [`error::AppError`] |
//! | [`money`] | 定点十进制金额 [`money::Amount`] |
//! | [`engine`] | 成交模拟与全部指标 |
//! | [`signal`] | 策略信号 [`Signal`] |
//! | [`bars`] | 策略可见的行情视图 [`Bars`] |
//! | [`types`] | 报告行与运行状态 |
//!
//! ## 源码内容哈希
//!
//! [`source_hash`] 由 `build.rs` 在构建期计算, 覆盖**全部 workspace 源码**、
//! 两个 path 依赖的 reader 与根 `Cargo.lock`。缓存指纹必须包含它, 否则
//! "改完代码再重跑"会命中旧缓存且无任何提示。

/// 策略可见的行情视图(六列, 生产路径保证等长)。
pub mod bars;
/// 回测引擎: 成交模拟(`simulate` 系列)与全部指标。
pub mod engine;
/// 统一错误类型([`error::AppError`])。
pub mod error;
/// 货币金额: 定点十进制([`money::Amount`]), 不用 `f64` 承载货币。
pub mod money;
/// 策略信号: 策略与引擎之间的唯一方向契约。
pub mod signal;
/// 报告行与运行状态的公开类型。
pub mod types;

pub use bars::Bars;
pub use signal::Signal;
pub use types::{DetailResult, RunResult, RunStatus};

/// 参与缓存指纹的源码内容哈希(构建期常量)。
///
/// 覆盖 `crates/*/src`、两个 reader 的 `src/` 与根 `Cargo.lock`。
#[must_use]
pub fn source_hash() -> &'static str {
    env!("VTA_SOURCE_HASH")
}
