//! # vta — 向量化技术指标回测框架
//!
//! 按 (品种 × 周期 × 策略 × 参数网格) 批量执行回测, 结果落 Parquet。
//!
//! 本 crate 只负责**装配**: CLI、配置、数据加载与整体编排。领域实现分布在
//! 4 个库 crate 中, 依赖方向单向向下:
//!
//! | crate | 职责 |
//! |---|---|
//! | `vta-core` | 错误、定点金额、回测引擎、信号与报告行类型 |
//! | `vta-strategies` | 策略实现与参数网格展开 |
//! | `vta-report` | Parquet 读写、报告列的单位契约 |
//! | `vta-pbo` | PBO via CSCV(过拟合概率) |
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`cli`] | 命令行参数解析 |
//! | [`config`] | TOML 配置的解析与语义校验(非法值在反序列化期即被拒) |
//! | [`data`] | 两个 reader 的统一入口 |
//! | [`app`] | 整体编排: 加载 → 逐组合回测 → 合并报告 |
//!
//! ## 三条核心不变量
//!
//! 1. **无前视偏差**: `delay_bars` 是 [`NonZeroUsize`](std::num::NonZeroUsize),
//!    类型上无法构造 0 —— 信号当根成交这种前视乐观口径已被移除。
//! 2. **指标与 `--detail` 开关无关**: `vta_core::engine::simulate` 与
//!    `simulate_detailed` 的指标都只经同一条在线累加器计算,
//!    且由类型系统保证指标无法读取 detailed 侧的落盘产物。
//! 3. **缓存指纹覆盖数据内容**: 缓存文件名混入行情的**内容哈希**而非路径/mtime,
//!    故补数据或原地替换数据后不会误命中旧缓存。
//!
//! ## 快速上手
//!
//! ```no_run
//! # fn main() -> vta_core::error::Result<()> {
//! // 跑一轮回测
//! vta::app::run(std::path::Path::new("config.toml"))?;
//! // 或实算 PBO
//! vta_pbo::run_pbo_report(std::path::Path::new("reports"))?;
//! # Ok(())
//! # }
//! ```

/// 整体编排: 加载配置 → 逐 (品种,周期,策略) 组合回测 → 合并为报告。
pub mod app;
/// 命令行参数解析(`clap`)。
pub mod cli;
/// TOML 配置: 解析、语义校验与策略参数网格定义。
pub mod config;
/// 数据加载适配层: vta 自己的配置/列结构与两个 reader 的统一入口。
pub mod data;
