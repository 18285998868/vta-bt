//! 策略配置: 单条策略、参数网格与范围参数。

use serde::Deserialize;

use vta_core::engine::StopConfig;
use vta_core::error::AppError;

use crate::registry::strategy_table;

/// 一条 `[[strategy]]` 条目: 策略参数 + **该策略专属**的持仓期止损。
///
/// 用 `#[serde(flatten)]` 而不是嵌套 `strategy = { ... }`: TOML 里保持
/// `type = "sma"` 平铺, 同时允许 `stop = { stop_loss_pct = 2.0 }` 或
/// `[strategy.stop]` 两种写法。`stop` 缺省为“无止损”。
///
/// 同一策略可以出现多次, 每次配不同的 `stop`, 从而形成 stop 参数网格;
/// `params` 会带止损后缀, 缓存指纹与报告行因此保持唯一。
#[derive(Debug, Clone, Deserialize)]
pub struct StrategySpec {
    /// 策略自身的配置(`type` + 参数)。
    #[serde(flatten)]
    pub strategy: StrategyConfig,
    /// 该策略的持仓期止损; 缺省无止损。
    #[serde(default)]
    pub stop: StopConfig,
    /// 该策略的仓位模式; 缺省全仓复利。
    #[serde(default)]
    pub sizing: vta_core::engine::SizingConfig,
}

/// Config for single-MA price-cross strategies.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaGridCfg {
    /// 均线周期, 单值或范围。
    pub period: PeriodParam,
}

impl MaGridCfg {
    /// 语义校验: 单周期策略只需校验 `period` 的区间与方向。
    /// 语义校验: 只需校验 `period` 的区间与方向。
    ///
    /// # Errors
    /// `period` 越界时返回 `AppError`。
    pub fn validate(&self) -> vta_core::error::Result<()> {
        self.period.validate()
    }
}

/// 由注册表生成 `StrategyConfig`: 枚举变体、TOML tag 与校验分派同源。
macro_rules! declare_strategy_config {
    ($(($module:ident, $tag:literal, $variant:ident, $config_module:ident, $config:ident, $strategy:ident, $kind:ident)),* $(,)?) => {
        /// 策略配置。以 TOML 的 `type` 标签做**内部标签**判别。
        ///
        /// 变体清单由 `registry.rs` 的 `strategy_table!` 生成;
        /// `deny_unknown_fields`: 拼错的键直接报错, 不会被静默忽略。
        #[derive(Debug, Clone, Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        pub enum StrategyConfig {
            $(
                #[doc = concat!("**", stringify!($variant), "** 策略: 配置 `", stringify!($config), "`, TOML `type = \"", $tag, "\"`。")]
                #[serde(rename = $tag)]
                $variant(crate::$config_module::$config),
            )*
        }

        impl StrategyConfig {
            /// 策略专属语义校验(参数范围与方向), 分派到各配置类型的 `validate`。
            ///
            /// # Errors
            /// 任一参数非法时返回 `AppError`。
            pub fn validate(&self) -> vta_core::error::Result<()> {
                match self {
                    $(Self::$variant(c) => c.validate(),)*
                }
            }
        }

        /// 全部策略的 TOML `type` 标签, 与 [`StrategyConfig`] 变体一一对应。
        pub const STRATEGY_TAGS: &[&str] = &[$($tag),*];
    };
}

strategy_table!(declare_strategy_config);

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 整型参数: 单值或范围。
pub enum PeriodParam {
    /// 单值形态。
    Single(usize),
    /// 范围形态 `{ start, end, step }`。
    Range(RangeParam<usize>),
}

impl PeriodParam {
    /// 语义校验: 范围必须 `start >= 1`、`start <= end`、`step > 0`。
    ///
    /// # Errors
    /// 任一约束不满足时返回 `AppError`。
    pub fn validate(&self) -> vta_core::error::Result<()> {
        self.validate_named("period", 1)
    }

    /// 语义校验, **参数名可定制**且允许非 1 的下界。
    ///
    /// 为什么需要它: 策略配置里除 `period` 外还有"步长型"整型参数
    /// (`sgma.poly_order` 多项式阶数), 它们的合法下界与字段名都和 `period`
    /// 不同。用 `validate()` 一刀切会误拒合法值
    /// (例如 `poly_order = 0`), 而**不校验**又会漏掉 `step = 0` ——
    /// 后者会在展开网格时触发 `Range::step_by(0)` 的 panic。
    ///
    /// # Errors
    /// 任一约束不满足时返回 `AppError`, 消息里带 `label` 便于定位。
    pub fn validate_named(&self, label: &str, min: usize) -> vta_core::error::Result<()> {
        match self {
            PeriodParam::Single(v) => {
                if *v < min {
                    return Err(AppError::Msg(format!("{label} 必须 >= {min}, 当前为 {v}")));
                }
            }
            PeriodParam::Range(r) => {
                if r.start < min {
                    return Err(AppError::Msg(format!(
                        "{label} 必须 >= {min}, 当前 start = {}",
                        r.start
                    )));
                }
                if r.start > r.end {
                    return Err(AppError::Msg(format!(
                        "{label} 起点 {} 不能大于终点 {}",
                        r.start, r.end
                    )));
                }
                if r.step == 0 {
                    return Err(AppError::Msg(format!(
                        "{label}.step 必须 > 0, 当前为 {}",
                        r.step
                    )));
                }
            }
        }
        Ok(())
    }

    /// 消耗自身归一化为范围(单值 → `start == end`)。
    #[must_use]
    pub fn into_range(self) -> RangeParam<usize> {
        match self {
            PeriodParam::Single(v) => RangeParam {
                start: v,
                end: v,
                step: 1,
            },
            PeriodParam::Range(r) => r,
        }
    }

    /// 借用自身归一化为范围(单值 → `start == end`)。
    #[must_use]
    pub fn to_range(&self) -> RangeParam<usize> {
        match self {
            PeriodParam::Single(v) => RangeParam {
                start: *v,
                end: *v,
                step: 1,
            },
            PeriodParam::Range(r) => r.clone(),
        }
    }

    /// 展开为具体取值列表。
    ///
    /// 上限 [`MAX_PERIOD_RANGE`] 档防爆(与 f64 展开的 `MAX_F64_RANGE` 同思路):
    /// 范围型参数的规模**无上限**(`start = 1, end = 10^9` ⇒ 10 亿档),
    /// 不封顶会把内存吃光。触顶时**告警**而非静默截断。
    #[must_use]
    pub fn values(&self) -> Vec<usize> {
        match self {
            PeriodParam::Single(v) => vec![*v],
            PeriodParam::Range(r) => {
                let mut out = Vec::new();
                // `step == 0` 会让下面的循环**不前进**(死循环)。`validate` 会
                // 单独拒绝它, 但本函数是 `pub` 的, 单独调用时也必须安全。
                if r.step == 0 {
                    return out;
                }
                let mut v = r.start;
                while v <= r.end && out.len() < MAX_PERIOD_RANGE {
                    out.push(v);
                    // `checked_add`: `v + step` 可能越过 `usize::MAX`。
                    match v.checked_add(r.step) {
                        Some(n) => v = n,
                        None => break,
                    }
                }
                if out.len() >= MAX_PERIOD_RANGE && v <= r.end {
                    tracing::warn!(
                        "period 范围展开触顶 {MAX_PERIOD_RANGE} 档(实际想展开到 {}), 已截断",
                        r.end
                    );
                }
                out
            }
        }
    }

    /// 校验展开结果落在 `[lo, hi]` 且非空。
    ///
    /// 与 [`F64Param::validate_range`] 同形, 但边界是**整数**。
    ///
    /// # Errors
    /// 展开为空或存在越界值时返回 `AppError`。
    pub fn validate_range(&self, name: &str, lo: usize, hi: usize) -> vta_core::error::Result<()> {
        let vs = self.values();
        if vs.is_empty() {
            return Err(AppError::Msg(format!("{name} 展开为空")));
        }
        if vs.iter().any(|v| *v < lo || *v > hi) {
            return Err(AppError::Msg(format!("{name} 越界(允许 [{lo}, {hi}])")));
        }
        Ok(())
    }
}

/// 整型参数展开的**档数上限**(防爆)。
///
/// 与 `crate::MAX_F64_RANGE` 同思路: 范围型参数的规模无上限,
/// 不封顶会把内存吃光。
pub const MAX_PERIOD_RANGE: usize = 100_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// 范围参数 `{ start, end, step }`。
pub struct RangeParam<T: PartialOrd + Copy> {
    /// 起点(含)。
    pub start: T,
    /// 终点(含)。
    pub end: T,
    /// 步长(恒 > 0, 由 `validate` 保证)。
    pub step: T,
}

/// f64 参数: 单值或范围(与 PeriodParam 对应, 后者只支持 usize)。
/// TOML 里 `k = 0.2` 与 `k = { start = 0.1, end = 0.6, step = 0.1 }` 都合法。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// 浮点参数: 单值或范围(与 [`PeriodParam`] 对应)。
pub enum F64Param {
    /// 单值形态。
    Single(f64),
    /// 范围形态 `{ start, end, step }`。
    Range(RangeParam<f64>),
}

impl F64Param {
    /// 展开为具体取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        match self {
            F64Param::Single(v) => vec![*v],
            F64Param::Range(r) => crate::expand_f64_range(r.start, r.end, r.step),
        }
    }

    /// 校验展开结果落在 `[lo, hi]` 且非空。
    ///
    /// # Errors
    /// 展开为空或存在越界/非有限值时返回 `AppError`。
    pub fn validate_range(&self, name: &str, lo: f64, hi: f64) -> vta_core::error::Result<()> {
        let vs = self.values();
        if vs.is_empty() {
            return Err(AppError::Msg(format!("{name} 展开为空")));
        }
        if vs.iter().any(|v| !v.is_finite() || *v < lo || *v > hi) {
            return Err(AppError::Msg(format!("{name} 越界(允许 [{lo}, {hi}])")));
        }
        Ok(())
    }
}
