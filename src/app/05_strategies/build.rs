use super::Strategy;
use super::aberration::AberrationStrategy;
use super::alma::AlmaStrategy;
use super::atma::AtmaStrategy;
use super::atrband::ATRBandStrategy;
use super::bbmr::BBMRStrategy;
use super::bbrk::BBRKStrategy;
use super::bbrk_adx::BbrkAdxStrategy;
use super::bbrk_atr::BbrkAtrStrategy;
use super::bbrk_atr_ema::BbrkAtrEmaStrategy;
use super::bbrk_bb::BbrkBbStrategy;
use super::bbrk_cross::BbrkCrossStrategy;
use super::bbrk_don::BbrkDonStrategy;
use super::bbrk_ema::BbrkEmaStrategy;
use super::bbrk_hma::BbrkHmaStrategy;
use super::bbrk_hma_ema::BbrkHmaEmaStrategy;
use super::bbrk_hma_v1::BbrkHmaV1Strategy;
use super::bbrk_hma_v2::BbrkHmaV2Strategy;
use super::bbrk_hma_v3::BbrkHmaV3Strategy;
use super::bbrk_hma_v4::BbrkHmaV4Strategy;
use super::bbrk_hma_v5::BbrkHmaV5Strategy;
use super::bbrk_kdj::BbrkKdjStrategy;
use super::bbrk_m2::BbrkM2Strategy;
use super::bbrk_m3::BbrkM3Strategy;
use super::bbrk_max::BbrkMaxStrategy;
use super::bbrk_orth::BbrkOrthStrategy;
use super::bbrk_pctb::BbrkPctbStrategy;
use super::bbrk_rex::BbrkRexStrategy;
use super::bbrk_sma::BbrkSmaStrategy;
use super::bbrk_sma_ema::BbrkSmaEmaStrategy;
use super::bbrk_str::BbrkStrStrategy;
use super::bbrk_trail::BbrkTrailStrategy;
use super::bbrk_ttm::BbrkTtmStrategy;
use super::bbrk_ttm_fire::BbrkTtmFireStrategy;
use super::bbrk_vol::BbrkVolStrategy;
use super::bbrk_vol_ema::BbrkVolEmaStrategy;
use super::bbrk_zlema_ema::BbrkZlemaEmaStrategy;
use super::clenow::ClenowStrategy;
use super::dema::DemaStrategy;
use super::dma::DmaStrategy;
use super::dualthrust::DualThrustStrategy;
use super::edma::EdmaStrategy;
use super::ehma::EhmaStrategy;
use super::ema::EmaStrategy;
use super::ema_cross::EmaCrossStrategy;
use super::fiali::FialiStrategy;
use super::frama::FramaStrategy;
use super::fwma::FwmaStrategy;
use super::hma::HmaStrategy;
use super::jma::JmaStrategy;
use super::kama::KamaStrategy;
use super::keltner::KeltnerStrategy;
use super::linreg::LinRegStrategy;
use super::ma_atr::MaAtrStrategy;
use super::ma_slope_atr_gate::MaSlopeAtrGateStrategy;
use super::mabb_long::MabbLongStrategy;
use super::mabb_short::MabbShortStrategy;
use super::macd::MacdStrategy;
use super::mcginley::McGinleyStrategy;
use super::pwma::PwmaStrategy;
use super::rbreaker::RBreakerStrategy;
use super::rsi2::RSI2Strategy;
use super::sgma::SgmaStrategy;
use super::sinwma::SinWmaStrategy;
use super::sma::SmaStrategy;
use super::sma_cd::SmaCdStrategy;
use super::smma::SmmaStrategy;
use super::ssf::SsfStrategy;
use super::ssf_cd::SsfCdStrategy;
use super::ssf_st::SsfStStrategy;
use super::ssf_st_cd::SsfStCdStrategy;
use super::swing_fractal::SwingFractalStrategy;
use super::swma::SwmaStrategy;
use super::tema::TemaStrategy;
use super::tilson::TilsonStrategy;
use super::trima::TrimaStrategy;
use super::turtle::TurtleStrategy;
use super::vidya::VidyaStrategy;
use super::vwap::VwapStrategy;
use super::vwap_trend::VwapTrendStrategy;
use super::vwma::VwmaStrategy;
use super::wilders::WildersStrategy;
use super::wma::WmaStrategy;
use super::zlema::ZlemaStrategy;
use crate::config::{StrategyConfig, StrategySpec};
use crate::engine::StopConfig;

/// 构建全部策略, 并对展开后的参数网格做**唯一性自检**。
///
/// # Errors
/// 任一策略在 `0..param_count()` 内出现下列任一情况时返回
/// [`AppError::Msg`](crate::error::AppError::Msg):
/// - `param_str(i)` 重复(网格展开有误, 或该策略的 param_str 未覆盖全部维度);
/// - **`strategy_params_value(param_str(i))` 重复** —— 即 parquet 的
///   `strategy_params` 整数列在同一策略内塌陷。
///
/// 背景: `strategy_params` 整数列由 `strategy_params_value` 把参数元组压缩而成。
/// 压缩**有损**: 槽位不够时回退 `fnv1a_32`(u32, 不同参数组可映射到同一值), 且
/// 即便未走 FNV 回退, 旧式槽位打包(`s*1M + h`)本身也会撞值 —— 例如
/// `sigma=1.5,bblen=60` 与 `sigma=2,bblen=10` 在 `h<1M` 的旧口径下同值。
///
/// 因此**只检查 `param_str` 字符串唯一是不够的**: 字符串可以两两不同, 压缩后的
/// 整数却撞在一起, 而报告排序/去重用的正是整数列。行主键实为
/// `(symbol, timeframe, strategy, params)` 四元组, 故撞值不构成数据丢失, 但会让
/// 按参数排序与 top-N 选取错乱且**完全静默**。两层都查, 把静默损坏变成构建期硬失败。
/// 代价是 O(N) 次 HashSet 插入(N ≤ 数万, 微秒级), 只在每次启动时跑一次。
pub fn build_strategies_checked(
    configs: &[StrategyConfig],
) -> crate::error::Result<Vec<Box<dyn Strategy>>> {
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
    pub sizing: crate::engine::SizingConfig,
}

/// 按 `[[strategy]]` 顺序构建策略 + 止损/仓位, 并逐条做参数网格唯一性自检。
///
/// 与 [`build_strategies_checked`] 的区别: 后者只接收策略配置, 用于不关心
/// 止损/仓位的测试/旧调用; 生产路径必须用本函数, 否则 `stop`/`sizing` 会丢失。
pub fn build_jobs_checked(specs: &[StrategySpec]) -> crate::error::Result<Vec<StrategyJob>> {
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
/// 1. `param_str` 字符串本身(网格是否展开正确);
/// 2. `strategy_params_value` 压缩后的整数(报告实际排序/去重所用的值)。
///
/// 第 2 项是**真正会塌陷的一层** —— 字符串唯一并不蕴含压缩后唯一。
fn check_param_uniqueness(strategies: &[Box<dyn Strategy>]) -> crate::error::Result<()> {
    for st in strategies {
        let n = st.param_count();
        let mut seen_str: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(n);
        // 值 → 产生它的参数串, 用于报错时直接指出是哪两组参数撞了。
        let mut seen_val: std::collections::HashMap<u64, String> =
            std::collections::HashMap::with_capacity(n);

        let mut dup_str: Option<String> = None;
        for i in 0..n {
            let p = st.param_str(i);
            if !seen_str.insert(p.clone()) && dup_str.is_none() {
                dup_str = Some(p.clone());
            }
            // 编码层: 同一 strategy 内不同参数串必须映射到不同整数。
            let v = crate::output::strategy_params_value(&p);
            if let Some(prev) = seen_val.insert(v, p.clone()) {
                return Err(crate::error::AppError::Msg(format!(
                    "[{}] strategy_params 编码碰撞: 参数组 {prev:?} 与 {p:?} 都编码为 {v} —— \
                     参数网格超出该编码的槽位容量, 会导致 parquet strategy_params 列 \
                     的排序与去重错乱(行主键是 params 列, 故数据不丢, 但选参会出错)",
                    st.name()
                )));
            }
        }
        if let Some(d) = dup_str {
            return Err(crate::error::AppError::Msg(format!(
                "[{}] 参数串重复: {d:?} —— 参数网格展开有误(或该策略的 param_str 未覆盖全部维度), \
                 会导致 parquet 的 strategy_params 列碰撞与选参结果错乱",
                st.name()
            )));
        }
    }
    Ok(())
}

/// 单个 `StrategyConfig` → 策略实例。**唯一的构造入口**。
///
/// 穷尽 `match`(刻意**不写 `_` 兜底**): 枚举的每一个变体都必须在此接上构造,
/// 新增变体而忘记接线会在**编译期**失败。
///
/// 返回 `Box<dyn Strategy>` 而非 `Option`: 不存在"构建失败"这一状态, 用
/// `Option` 只会让调用方的 `filter_map` 静默吞掉漏接的变体。
///
/// 原实现有三处**手工同步**的变体清单 —— `build_ma!` 宏的 `unreachable!()` 兜底、
/// `build_strategies` 里的 `matches!` 排除表、以及 60 条 `filter_map` 分支。漏改前两处
/// 不会报错, 只会**静默跳过**该策略(配置里写了却不跑)。现在只剩这一处, 且由编译器兜底。
fn build_one(sc: &StrategyConfig) -> Box<dyn Strategy> {
    match sc {
        // ---- 单 MA 周期族: config 统一是 MaGridCfg, 只用到 period ----
        StrategyConfig::Alma(c) => Box::new(AlmaStrategy::new(c.period.to_range())),
        StrategyConfig::Dema(c) => Box::new(DemaStrategy::new(c.period.to_range())),
        StrategyConfig::Dma(c) => Box::new(DmaStrategy::new(c.period.to_range())),
        StrategyConfig::Ehma(c) => Box::new(EhmaStrategy::new(c.period.to_range())),
        StrategyConfig::Frama(c) => Box::new(FramaStrategy::new(c.period.to_range())),
        StrategyConfig::Smma(c) => Box::new(SmmaStrategy::new(c.period.to_range())),
        StrategyConfig::Ema(c) => Box::new(EmaStrategy::new(c.period.to_range())),
        StrategyConfig::Fwma(c) => Box::new(FwmaStrategy::new(c.period.to_range())),
        StrategyConfig::Hma(c) => Box::new(HmaStrategy::new(c.period.to_range())),
        StrategyConfig::Jma(c) => Box::new(JmaStrategy::new(c.period.to_range())),
        StrategyConfig::Kama(c) => Box::new(KamaStrategy::new(c.period.to_range())),
        StrategyConfig::LinReg(c) => Box::new(LinRegStrategy::new(c.period.to_range())),
        StrategyConfig::McGinley(c) => Box::new(McGinleyStrategy::new(c.period.to_range())),
        StrategyConfig::Pwma(c) => Box::new(PwmaStrategy::new(c.period.to_range())),
        StrategyConfig::SinWma(c) => Box::new(SinWmaStrategy::new(c.period.to_range())),
        StrategyConfig::Sma(c) => Box::new(SmaStrategy::new(c.period.to_range())),
        StrategyConfig::Swma(c) => Box::new(SwmaStrategy::new(c.period.to_range())),
        StrategyConfig::Tema(c) => Box::new(TemaStrategy::new(c.period.to_range())),
        StrategyConfig::Tilson(c) => Box::new(TilsonStrategy::new(c.period.to_range())),
        StrategyConfig::Trima(c) => Box::new(TrimaStrategy::new(c.period.to_range())),
        StrategyConfig::Vidya(c) => Box::new(VidyaStrategy::new(c.period.to_range())),
        StrategyConfig::Vwma(c) => Box::new(VwmaStrategy::new(c.period.to_range())),
        StrategyConfig::Wilders(c) => Box::new(WildersStrategy::new(c.period.to_range())),
        StrategyConfig::Wma(c) => Box::new(WmaStrategy::new(c.period.to_range())),
        StrategyConfig::Zlma(c) => Box::new(ZlemaStrategy::new(c.period.to_range())),

        // ---- 其余策略: 各自持有专属 config ----
        StrategyConfig::Ssf(c) => Box::new(SsfStrategy::new(c.clone())),
        StrategyConfig::SsfCd(c) => Box::new(SsfCdStrategy::new(c.clone())),
        StrategyConfig::SmaCd(c) => Box::new(SmaCdStrategy::new(c.clone())),
        StrategyConfig::SsfSt(c) => Box::new(SsfStStrategy::new(c.clone())),
        StrategyConfig::SsfStCd(c) => Box::new(SsfStCdStrategy::new(c.clone())),
        StrategyConfig::SwingFractal(c) => Box::new(SwingFractalStrategy::new(c.clone())),
        StrategyConfig::EmaCross(c) => Box::new(EmaCrossStrategy::new(c.clone())),
        StrategyConfig::Turtle(c) => Box::new(TurtleStrategy::new(c.clone())),
        StrategyConfig::Clenow(c) => Box::new(ClenowStrategy::new(c.clone())),
        StrategyConfig::DualThrust(c) => Box::new(DualThrustStrategy::new(c.clone())),
        StrategyConfig::RBreaker(c) => Box::new(RBreakerStrategy::new(c.clone())),
        StrategyConfig::Fiali(c) => Box::new(FialiStrategy::new(c.clone())),
        StrategyConfig::Edma(c) => Box::new(EdmaStrategy::new(c.clone())),
        StrategyConfig::Atma(c) => Box::new(AtmaStrategy::new(c.clone())),
        StrategyConfig::Sgma(c) => Box::new(SgmaStrategy::new(c.clone())),
        StrategyConfig::Aberration(c) => Box::new(AberrationStrategy::new(c.clone())),
        StrategyConfig::ATRBand(c) => Box::new(ATRBandStrategy::new(c.clone())),
        StrategyConfig::Keltner(c) => Box::new(KeltnerStrategy::new(c.clone())),
        StrategyConfig::BBMR(c) => Box::new(BBMRStrategy::new(c.clone())),
        StrategyConfig::Bbrk(c) => Box::new(BBRKStrategy::new(c.clone())),
        StrategyConfig::BbrkAdx(c) => Box::new(BbrkAdxStrategy::new(c.clone())),
        StrategyConfig::BbrkTrail(c) => Box::new(BbrkTrailStrategy::new(c.clone())),
        StrategyConfig::BbrkDon(c) => Box::new(BbrkDonStrategy::new(c.clone())),
        StrategyConfig::BbrkEma(c) => Box::new(BbrkEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkPctb(c) => Box::new(BbrkPctbStrategy::new(c.clone())),
        StrategyConfig::BbrkAtr(c) => Box::new(BbrkAtrStrategy::new(c.clone())),
        StrategyConfig::BbrkStr(c) => Box::new(BbrkStrStrategy::new(c.clone())),
        StrategyConfig::BbrkSma(c) => Box::new(BbrkSmaStrategy::new(c.clone())),
        StrategyConfig::BbrkHma(c) => Box::new(BbrkHmaStrategy::new(c.clone())),
        StrategyConfig::BbrkMax(c) => Box::new(BbrkMaxStrategy::new(c.clone())),
        StrategyConfig::BbrkBb(c) => Box::new(BbrkBbStrategy::new(c.clone())),
        StrategyConfig::BbrkVolEma(c) => Box::new(BbrkVolEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkAtrEma(c) => Box::new(BbrkAtrEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkSmaEma(c) => Box::new(BbrkSmaEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkHmaEma(c) => Box::new(BbrkHmaEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkHmaV1(c) => Box::new(BbrkHmaV1Strategy::new(c.clone())),
        StrategyConfig::BbrkHmaV2(c) => Box::new(BbrkHmaV2Strategy::new(c.clone())),
        StrategyConfig::BbrkHmaV3(c) => Box::new(BbrkHmaV3Strategy::new(c.clone())),
        StrategyConfig::BbrkHmaV4(c) => Box::new(BbrkHmaV4Strategy::new(c.clone())),
        StrategyConfig::BbrkHmaV5(c) => Box::new(BbrkHmaV5Strategy::new(c.clone())),
        StrategyConfig::BbrkZlemaEma(c) => Box::new(BbrkZlemaEmaStrategy::new(c.clone())),
        StrategyConfig::BbrkRex(c) => Box::new(BbrkRexStrategy::new(c.clone())),
        StrategyConfig::BbrkVol(c) => Box::new(BbrkVolStrategy::new(c.clone())),
        StrategyConfig::BbrkTtm(c) => Box::new(BbrkTtmStrategy::new(c.clone())),
        StrategyConfig::BbrkTtmFire(c) => Box::new(BbrkTtmFireStrategy::new(c.clone())),
        StrategyConfig::BbrkCross(c) => Box::new(BbrkCrossStrategy::new(c.clone())),
        StrategyConfig::BbrkKdj(c) => Box::new(BbrkKdjStrategy::new(c.clone())),
        StrategyConfig::BbrkM2(c) => Box::new(BbrkM2Strategy::new(c.clone())),
        StrategyConfig::BbrkM3(c) => Box::new(BbrkM3Strategy::new(c.clone())),
        StrategyConfig::BbrkOrth(c) => Box::new(BbrkOrthStrategy::new(c.clone())),
        StrategyConfig::VwapTrend(c) => Box::new(VwapTrendStrategy::new(c.clone())),
        StrategyConfig::RSI2(c) => Box::new(RSI2Strategy::new(c.clone())),
        StrategyConfig::MabbLong(c) => Box::new(MabbLongStrategy::new(c.clone())),
        StrategyConfig::MabbShort(c) => Box::new(MabbShortStrategy::new(c.clone())),
        StrategyConfig::MaAtr(c) => Box::new(MaAtrStrategy::new(c.clone())),
        StrategyConfig::MaSlopeAtrGate(c) => Box::new(MaSlopeAtrGateStrategy::new(c.clone())),
        StrategyConfig::Macd(c) => Box::new(MacdStrategy::new(c.clone())),
        StrategyConfig::Vwap(c) => Box::new(VwapStrategy::new(c.clone())),
    }
}

/// 按配置顺序构建策略实例(不含 stop)。
///
/// 生产路径请用 [`build_jobs_checked`], 它保留每条 `[[strategy]]` 的 stop,
/// 并额外做参数网格唯一性自检; 本函数只供不关心 stop 的测试/旧调用。
#[must_use]
pub fn build_strategies(configs: &[StrategyConfig]) -> Vec<Box<dyn Strategy>> {
    configs.iter().map(build_one).collect()
}
