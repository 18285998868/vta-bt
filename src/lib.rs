//! TradingView backend: root support modules + `app/` pipeline.

pub mod app;

pub mod config;
pub mod error;
pub mod utils;

pub use app::run;
pub use error::AppError;
