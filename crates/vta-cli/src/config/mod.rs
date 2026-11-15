//! 配置: 原始 TOML 结构 → 校验 → 类型化配置。

mod backtest;
mod data;
mod load;
mod output;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use backtest::{BacktestConfig, FillBarPrice, InitialBalance};
pub use data::{DataSection, OneOrMany};
pub use load::load_config;
pub use output::OutputConfig;

use serde::Deserialize;

use vta_core::error::AppError;
use vta_strategies::StrategySpec;

use output::check_path_safe_component;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// 回测配置根。`deny_unknown_fields`: 拼错的键会直接报错而非被忽略。
pub struct Config {
    /// 数据源与扫描范围(品种 × 周期)。
    pub data: DataSection,
    /// 执行口径(资金 / 延迟 / 费率)。
    pub backtest: BacktestConfig,
    /// 输出选项; 缺省写到 `./reports/`。
    pub output: Option<OutputConfig>,
    /// 要跑的策略列表(至少一个); 每条可带独立止损。
    pub strategy: Vec<StrategySpec>,
}

impl Config {
    /// 语义校验。**执行口径(资金/费率/延迟)已由 [`BacktestConfig`] 的字段类型
    /// 在反序列化时保证**, 此处不再重复检查; 只剩数据维度与策略专属约束。
    /// 语义校验。
    ///
    /// # Errors
    /// 品种/周期为空、**品种/周期含路径穿越字符**、周期未知或策略参数非法时
    /// 返回 `AppError`。
    pub fn validate(&self) -> vta_core::error::Result<()> {
        let syms = self.data.symbols();
        if syms.is_empty() || syms.iter().any(String::is_empty) {
            return Err(AppError::Msg("data.symbol 不能为空".into()));
        }
        // 品种名会**直接进入落盘文件名**(`{symbol}_{tf}_{strategy}_…parquet`),
        // 故必须在这里挡住路径穿越 —— 否则 `symbol = "../evil"` 会让产物写到
        // 输出目录的**上一层**。真实品种名都是字母数字(`BTCUSDT` /
        // `1000SHIBUSDT` / `1INCHUSDT`), 白名单不会误伤。
        for s in syms {
            check_path_safe_component("data.symbol", s)?;
        }
        let tfs = self.data.timeframes();
        if tfs.is_empty() || tfs.iter().any(String::is_empty) {
            return Err(AppError::Msg("data.timeframe 不能为空".into()));
        }
        for tf in tfs {
            // 周期同理: 它也进文件名(`{symbol}_{tf}_…`)。
            check_path_safe_component("data.timeframe", tf)?;
            if !vta_strategies::is_known_timeframe(tf) {
                return Err(AppError::Msg(format!(
                    "未知周期 data.timeframe = {tf:?}; 已知: 1m/3m/5m/15m/30m/1h/2h/4h/6h/8h/12h/1d/1w"
                )));
            }
        }
        // `fixed_curves` 只在 detail 分支写出。若允许它单独为 true, 用户会得到
        // "配置说要做 PBO, 但 detail/ 一个 fixedcurve 都没有" 的静默空结果 ——
        // 随后 `--pbo` 报"未找到曲线", 排查方向完全错。这里直接拒绝。
        if self
            .output
            .as_ref()
            .is_some_and(|output| output.fixed_curves && !output.detail)
        {
            return Err(AppError::Msg(
                "output.fixed_curves = true 需要同时设置 output.detail = true: \
                 fixedcurve 只在 detail 分支写出"
                    .into(),
            ));
        }
        if self.strategy.is_empty() {
            return Err(AppError::Msg("strategy 不能为空".into()));
        }
        for spec in &self.strategy {
            spec.stop.validate()?;
            spec.sizing.validate()?;
            // 策略专属校验: 下沉到 `StrategyConfig::validate`(由注册表生成),
            // 此处只负责调用 —— 不再维护 81 臂手写分派。
            spec.strategy.validate()?;
        }
        Ok(())
    }
}
