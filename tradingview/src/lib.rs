//! TradingView 后端：本地 binance zip → K 线 REST 服务 + 策略回测。
//!
//! 分层：
//! - [`app`]：HTTP 编排（[`app::run`]）、路由、行情加载、策略回测。
//! - [`config`]：固定路径 TOML 配置与语义校验。
//! - [`error`]：库层统一错误类型 [`AppError`]（`thiserror`）。
//! - [`utils`]：入参白名单与目录探测。
//!
//! 入口（`src/main.rs`）只做三件事：装日志 → 读配置 → 启服务。

#![doc = include_str!("../README.md")]
#![deny(clippy::correctness)]
#![deny(clippy::suspicious)]
#![warn(clippy::style)]
#![warn(clippy::complexity)]
#![warn(clippy::perf)]
#![warn(missing_docs)]

pub mod app;

pub mod config;
pub mod error;
pub mod utils;

pub use app::run;
pub use error::AppError;
