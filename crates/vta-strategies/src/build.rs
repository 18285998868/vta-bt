use super::Strategy;
use crate::config::{StrategyConfig, StrategySpec};
use crate::registry::strategy_table;
use vta_core::engine::StopConfig;

/// 构建全部策略, 并对展开后的参数网格做**唯一性自检**。
///
/// # Errors
/// 任一策略在 `0..param_count()` 内出现下列任一情况时返回
/// [`AppError::Msg`](vta_core::error::AppError::Msg):
/// - `param_set(i).text()` 重复(网格展开有误, 或该策略的 `ParamSet` 未覆盖全部维度);
/// - **`param_set(i).code()` 重复** —— 即 parquet 的 `strategy_params` 整数列在
///   同一策略内塌陷。
///
/// 背景: `strategy_params` 整数列由策略产出的 [`ParamSet`](crate::ParamSet) 给出。
/// 定宽打包在越界时会回退字段哈希, 旧式槽位公式也可能撞值; 因此**只检查文本唯一
/// 是不够的**: 字符串可以两两不同, 整数却撞在一起, 而报告排序/去重用的正是整数列。
/// 行主键实为 `(symbol, timeframe, strategy, params)` 四元组, 故撞值不构成数据丢失,
/// 但会让按参数排序与 top-N 选取错乱且**完全静默**。两层都查, 把静默损坏变成构建期
/// 硬失败。代价是 O(N) 次 HashSet 插入(N ≤ 数万, 微秒级), 只在每次启动时跑一次。
pub fn build_strategies_checked(
    configs: &[StrategyConfig],
) -> vta_core::error::Result<Vec<Box<dyn Strategy>>> {
    let strategies = build_strategies(configs);
    check_param_uniqueness(&strategies)?;
    Ok(strategies)
}

/// 一个待回测单元: 策略实例 + 该条 `[[strategy]]` 的止损/仓位配置。
///
/// 止损与仓位是**运行参数**, 不是策略内部状态: 同一策略实例可以配不同
/// stop/sizing 重复运行, 策略代码无需知道它们存在。
pub struct StrategyJob {
    /// 已构建的策略实例。
    pub strategy: Box<dyn Strategy>,
    /// 该条策略的持仓期止损。
    pub stop: StopConfig,
    /// 该条策略的仓位模式。
    pub sizing: vta_core::engine::SizingConfig,
}

/// 按 `[[strategy]]` 顺序构建策略 + 止损/仓位, 并逐条做参数网格唯一性自检。
///
/// 与 [`build_strategies_checked`] 的区别: 后者只接收策略配置, 用于不关心
/// 止损/仓位的测试/旧调用; 生产路径必须用本函数, 否则 `stop`/`sizing` 会丢失。
pub fn build_jobs_checked(specs: &[StrategySpec]) -> vta_core::error::Result<Vec<StrategyJob>> {
    let mut jobs = Vec::with_capacity(specs.len());
    for spec in specs {
        spec.stop.validate()?;
        spec.sizing.validate()?;
        let strategy = build_one(&spec.strategy);
        check_param_uniqueness(std::slice::from_ref(&strategy))?;
        jobs.push(StrategyJob {
            strategy,
            stop: spec.stop,
            sizing: spec.sizing,
        });
    }
    Ok(jobs)
}

/// 逐策略检查两件事在参数网格内均两两不同:
/// 1. `ParamSet::text` 文本本身(网格是否展开正确);
/// 2. `ParamSet::code` 整数(报告实际排序/去重所用的值)。
///
/// 第 2 项是**真正会塌陷的一层** —— 文本唯一并不蕴含编码唯一。
fn check_param_uniqueness(strategies: &[Box<dyn Strategy>]) -> vta_core::error::Result<()> {
    for st in strategies {
        let n = st.param_count();
        let mut seen_str: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(n);
        // 值 → 产生它的参数串, 用于报错时直接指出是哪两组参数撞了。
        let mut seen_val: std::collections::HashMap<u64, String> =
            std::collections::HashMap::with_capacity(n);

        let mut dup_str: Option<String> = None;
        for i in 0..n {
            let param = st.param_set(i);
            let p = param.text().to_string();
            if !seen_str.insert(p.clone()) && dup_str.is_none() {
                dup_str = Some(p.clone());
            }
            // 编码层: 同一 strategy 内不同参数组必须映射到不同整数。
            let v = param.code();
            if let Some(prev) = seen_val.insert(v, p.clone()) {
                return Err(vta_core::error::AppError::Msg(format!(
                    "[{}] strategy_params 编码碰撞: 参数组 {prev:?} 与 {p:?} 都编码为 {v} —— \
                     参数网格超出该编码的槽位容量, 会导致 parquet strategy_params 列 \
                     的排序与去重错乱(行主键是 params 列, 故数据不丢, 但选参会出错)",
                    st.name()
                )));
            }
        }
        if let Some(d) = dup_str {
            return Err(vta_core::error::AppError::Msg(format!(
                "[{}] 参数串重复: {d:?} —— 参数网格展开有误(或该策略的 param_set 未覆盖全部维度), \
                 会导致 parquet 的 strategy_params 列碰撞与选参结果错乱",
                st.name()
            )));
        }
    }
    Ok(())
}

/// 由注册表生成**唯一的构造入口**: 每个 `StrategyConfig` 变体恰好一条臂。
///
/// 穷尽 `match`(不写 `_` 兜底)由同一张表保证: 新增变体必然多出一行,
/// 也就必然同时多出一条构造臂 —— 不存在"配置里写了却不跑"的静默跳过。
///
/// 返回 `Box<dyn Strategy>` 而非 `Option`: 不存在"构建失败"这一状态, 用
/// `Option` 只会让调用方的 `filter_map` 静默吞掉漏接的变体。
macro_rules! declare_build_one {
    ($(($module:ident, $tag:literal, $variant:ident, $config_module:ident, $config:ident, $strategy:ident, $kind:ident)),* $(,)?) => {
        fn build_one(sc: &StrategyConfig) -> Box<dyn Strategy> {
            match sc {
                $(
                    StrategyConfig::$variant(c) => {
                        declare_build_one!(@build $kind, $module, $strategy, c)
                    }
                )*
            }
        }
    };
    (@build period, $module:ident, $strategy:ident, $c:ident) => {
        Box::new(crate::$module::$strategy::new($c.period.to_range()))
    };
    (@build clone, $module:ident, $strategy:ident, $c:ident) => {
        Box::new(crate::$module::$strategy::new($c.clone()))
    };
}

strategy_table!(declare_build_one);

/// 按配置顺序构建策略实例(不含 stop)。
///
/// 生产路径请用 [`build_jobs_checked`], 它保留每条 `[[strategy]]` 的 stop,
/// 并额外做参数网格唯一性自检; 本函数只供不关心 stop 的测试/旧调用。
#[must_use]
pub fn build_strategies(configs: &[StrategyConfig]) -> Vec<Box<dyn Strategy>> {
    configs.iter().map(build_one).collect()
}
