//! # vta — 向量化技术指标回测框架
//!
//! 按 (品种 × 周期 × 策略 × 参数网格) 批量执行回测, 结果落 Parquet。
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`cli`] | 命令行参数解析 |
//! | [`config`] | TOML 配置的解析与语义校验(非法值在反序列化期即被拒) |
//! | [`engine`] | 回测引擎: 成交模拟与全部指标 |
//! | [`strategies`] | 策略实现与参数网格展开 |
//! | [`output`] | Parquet 读写、报告列的单位契约 |
//! | [`pbo`] | PBO via CSCV(过拟合概率) |
//! | [`app`] | 整体编排: 加载 → 逐组合回测 → 合并报告 |
//!
//! ## 三条核心不变量
//!
//! 1. **无前视偏差**: `delay_bars` 是 [`NonZeroUsize`](std::num::NonZeroUsize),
//!    类型上无法构造 0 —— 信号当根成交这种前视乐观口径已被移除。
//! 2. **指标与 `--detail` 开关无关**: [`engine::simulate`] 与
//!    [`engine::simulate_detailed`] 的指标都只经同一条在线累加器计算,
//!    且由类型系统保证指标无法读取 detailed 侧的落盘产物。
//! 3. **缓存指纹覆盖数据内容**: 缓存文件名混入行情的**内容哈希**而非路径/mtime,
//!    故补数据或原地替换数据后不会误命中旧缓存。
//!
//! ## 快速上手
//!
//! ```no_run
//! # fn main() -> vta::error::Result<()> {
//! // 跑一轮回测
//! vta::app::run(std::path::Path::new("config.toml"))?;
//! // 或实算 PBO
//! vta::pbo::run_pbo_report(std::path::Path::new("reports"))?;
//! # Ok(())
//! # }
//! ```

/// 整体编排: 加载配置 → 逐 (品种,周期,策略) 组合回测 → 合并为报告。
pub mod app;
/// 命令行参数解析(`clap`)。
pub mod cli;
/// TOML 配置: 解析、语义校验与策略参数网格定义。
pub mod config;
/// 回测引擎: 成交模拟(`simulate` 系列)与全部指标。
#[path = "app/03_run_backtest.rs"]
pub mod engine;
/// 统一错误类型([`error::AppError`])。
pub mod error;
/// 货币金额: 定点十进制([`money::Amount`]), 不用 `f64` 承载货币。
pub mod money;
/// Parquet 读写层。
#[path = "app/04_write_output/mod.rs"]
pub mod output;
/// PBO via CSCV: 由已落盘的权益曲线实算过拟合概率。
pub mod pbo;
/// 策略实现与参数网格展开。
#[path = "app/05_strategies/mod.rs"]
pub mod strategies;
