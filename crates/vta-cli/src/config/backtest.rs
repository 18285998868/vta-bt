//! 回测执行配置: 资金、延迟、费率与成交价。

use std::num::NonZeroUsize;

use chrono::NaiveDate;
use serde::Deserialize;

use vta_core::engine::Direction;
use vta_core::error::AppError;

/// 初始资金: **严格正**, 且是**定点十进制金额**。
///
/// 用 newtype 而非裸 `f64` + 运行时 `if`: 后者让 `balance = 0` 成为可表示的状态,
/// 而它在下游会触发 `qty = cash / entry_price` 与 `fixed_qty = initial_cash / entry`
/// 的除零。现在该值**无法用非法状态表示**。
///
/// 内部用 [`Amount`](vta_core::money::Amount) 而非 `f64` ——
/// 资金是**货币**, `domain-fintech` 的硬规则是"Never use `f64` for money"。
/// 此前本 newtype 只是给 `f64` 套了个名字: `get()` 之后 `balance: f64` 一路流通,
/// 任何人都能直接构造 `f64` 绕过校验 —— 那是**假**的类型安全。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct InitialBalance(vta_core::money::Amount);

impl InitialBalance {
    /// 金额值(定点十进制)。
    #[must_use]
    pub fn amount(self) -> vta_core::money::Amount {
        self.0
    }

    /// 取回 `f64` 形式(**仅供统计/落盘口径**, 内部金额运算不得用)。
    #[must_use]
    pub fn get(self) -> f64 {
        self.0.to_f64()
    }
}

impl TryFrom<f64> for InitialBalance {
    type Error = AppError;

    fn try_from(v: f64) -> vta_core::error::Result<Self> {
        if v > 0.0 && v.is_finite() {
            Ok(Self(vta_core::money::Amount::from_f64(v)?))
        } else {
            Err(AppError::Msg(format!(
                "backtest.initial_balance 必须 > 0 且为有限值, 当前为 {v}"
            )))
        }
    }
}

/// 单边费率(bp): **非负且有限**。负费率(返佣)当前未被任何语义支持, 故一并拒绝。
macro_rules! nonneg_bps {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        /// 单边费率(bp): **非负且有限**。见模块内 `nonneg_bps` 宏的说明。
        pub struct $name(f64);

        impl $name {
            /// 取回原始数值。
            #[must_use]
            pub fn get(self) -> f64 {
                self.0
            }
        }

        impl TryFrom<f64> for $name {
            type Error = AppError;

            fn try_from(v: f64) -> vta_core::error::Result<Self> {
                if v >= 0.0 && v.is_finite() {
                    Ok(Self(v))
                } else {
                    Err(AppError::Msg(format!(
                        "backtest.{} 不能为负且须为有限值, 当前为 {v}",
                        $label
                    )))
                }
            }
        }
    };
}
nonneg_bps!(FeeBps, "fee_bps");
nonneg_bps!(SlippageBps, "slippage_bps");

/// 缺省延迟 1 bar (次根 fill bar 成交)。`delay_bars = 0` 由 [`NonZeroUsize`]
/// 在构造期拒绝。
pub(super) fn default_delay_bars() -> usize {
    1
}

/// 反序列化中间层: 全部字段为裸类型 + serde `default`。
/// 只做**语法**解析(缺省填充), 语义合法性交给 [`TryFrom`]。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawBacktestConfig {
    initial_balance: f64,
    #[serde(default = "default_delay_bars")]
    delay_bars: usize,
    #[serde(default)]
    fee_bps: f64,
    #[serde(default)]
    slippage_bps: f64,
    #[serde(default)]
    fill_bar_price: FillBarPrice,
    #[serde(default)]
    direction: Direction,
    #[serde(default)]
    trade_start: Option<String>,
}

/// 回测执行配置。**字段类型保证值合法**, 无需事后 `validate()`:
/// `try_from` 使非法 TOML 在**反序列化时**就报错(自带行列号), 而不是在 `run()`
/// 里被逐条 `if` 拦下。
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawBacktestConfig")]
pub struct BacktestConfig {
    /// 初始资金: 严格正且有限。
    pub initial_balance: InitialBalance,
    /// 信号到成交的 bar 间隔, 恒 ≥ 1。`delay_bars = 0`(信号当根成交, 前视乐观)
    /// 已在引擎层彻底移除, 此处类型级拒绝。
    pub delay_bars: NonZeroUsize,
    /// 单边佣金(bp): 非负且有限。
    pub fee_bps: FeeBps,
    /// 单边逆向滑点(bp): 非负且有限。
    pub slippage_bps: SlippageBps,
    /// fill bar 的成交价: "open"(默认, 次根开盘)或 "close"(次根收盘)。
    pub fill_bar_price: FillBarPrice,
    /// 允许开仓的方向: `both`(默认) / `long_only` / `short_only`。
    ///
    /// 这是**引擎层**的过滤, 与策略参数无关: 被禁方向的开仓信号按 `Hold`
    /// 处理(不反手、不平仓), `Close` / `ExitLong` / `ExitShort` 不受影响。
    /// 同一组多空信号因此可以用配置切换到 long-only 口径做对照。
    pub direction: Direction,
    /// 交易起点(含): 此日期之前的 bar 只用于预热指标/策略状态, 不产生成交。
    ///
    /// 与 `[data].start_date` 的区别: `start_date` 是**数据加载**边界, 指标会
    /// 从该日冷启动; `trade_start` 保留更早的数据做暖机, 回测从该日正式开始,
    /// 起始日空仓。研究脚本"全历史算指标 + start 日重置状态"的口径即此语义。
    /// 缺省 `None` 保持旧行为。
    pub trade_start: Option<NaiveDate>,
}

impl TryFrom<RawBacktestConfig> for BacktestConfig {
    type Error = AppError;

    fn try_from(r: RawBacktestConfig) -> vta_core::error::Result<Self> {
        let trade_start = r
            .trade_start
            .as_deref()
            .map(|raw| {
                NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| {
                    AppError::Msg(format!(
                        "backtest.trade_start 须为 YYYY-MM-DD, 当前为 {raw:?}"
                    ))
                })
            })
            .transpose()?;
        Ok(Self {
            initial_balance: InitialBalance::try_from(r.initial_balance)?,
            delay_bars: NonZeroUsize::new(r.delay_bars).ok_or_else(|| {
                AppError::Msg(
                    "backtest.delay_bars 必须 >= 1: 信号当根成交为前视乐观口径, 已移除".to_string(),
                )
            })?,
            fee_bps: FeeBps::try_from(r.fee_bps)?,
            slippage_bps: SlippageBps::try_from(r.slippage_bps)?,
            fill_bar_price: r.fill_bar_price,
            direction: r.direction,
            trade_start,
        })
    }
}

/// fill bar 的成交价位 (delay恒≥1)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FillBarPrice {
    /// fill bar 开盘(默认, 现状口径)。
    #[default]
    Open,
    /// fill bar 收盘(多等一根 bar 内漂移, 更贴近追单成交)。
    Close,
}
