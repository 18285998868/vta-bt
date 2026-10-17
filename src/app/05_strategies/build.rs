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
use super::bbrk_hma_v1_sl::BbrkHmaV1SlStrategy;
use super::bbrk_hma_v2::BbrkHmaV2Strategy;
use super::bbrk_hma_v2_sl::BbrkHmaV2SlStrategy;
use super::bbrk_hma_v3::BbrkHmaV3Strategy;
use super::bbrk_hma_v3_sl::BbrkHmaV3SlStrategy;
use super::bbrk_hma_v4::BbrkHmaV4Strategy;
use super::bbrk_hma_v4_sl::BbrkHmaV4SlStrategy;
use super::bbrk_hma_v5::BbrkHmaV5Strategy;
use super::bbrk_hma_v5_sl::BbrkHmaV5SlStrategy;
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
use crate::config::StrategyConfig;

macro_rules! build_ma {
    ($cfg:expr, $($variant:ident => $strategy:ty),*) => {
        match $cfg {
            $(StrategyConfig::$variant(c) => {
                std::iter::once(Box::new(<$strategy>::new(c.period.to_range())) as Box<dyn Strategy>)
            })*
            StrategyConfig::Ssf(_)
            | StrategyConfig::SsfCd(_)
            | StrategyConfig::SsfSt(_)
            | StrategyConfig::SsfStCd(_)
            | StrategyConfig::SmaCd(_)
            | StrategyConfig::EmaCross(_)
            | StrategyConfig::Turtle(_)
            | StrategyConfig::DualThrust(_)
            | StrategyConfig::RBreaker(_)
            | StrategyConfig::Fiali(_)
            | StrategyConfig::Aberration(_)
            | StrategyConfig::ATRBand(_)
            | StrategyConfig::Keltner(_)
            | StrategyConfig::BBMR(_)
            | StrategyConfig::Bbrk(_)
            | StrategyConfig::BbrkAdx(_)
            | StrategyConfig::BbrkTrail(_)
            | StrategyConfig::BbrkDon(_)
            | StrategyConfig::BbrkEma(_)
            | StrategyConfig::BbrkPctb(_)
            | StrategyConfig::BbrkAtr(_)
            | StrategyConfig::BbrkStr(_)
            | StrategyConfig::BbrkSma(_)
            | StrategyConfig::BbrkHma(_)
            | StrategyConfig::BbrkMax(_)
            | StrategyConfig::BbrkBb(_)
            | StrategyConfig::BbrkVolEma(_)
            | StrategyConfig::BbrkAtrEma(_)
            | StrategyConfig::BbrkSmaEma(_)
            | StrategyConfig::BbrkHmaEma(_)
            | StrategyConfig::BbrkHmaV1(_)
            | StrategyConfig::BbrkHmaV1Sl(_)
            | StrategyConfig::BbrkHmaV2(_)
            | StrategyConfig::BbrkHmaV2Sl(_)
            | StrategyConfig::BbrkHmaV3(_)
            | StrategyConfig::BbrkHmaV3Sl(_)
            | StrategyConfig::BbrkHmaV4(_)
            | StrategyConfig::BbrkHmaV4Sl(_)
            | StrategyConfig::BbrkHmaV5(_)
            | StrategyConfig::BbrkHmaV5Sl(_)
            | StrategyConfig::BbrkZlemaEma(_)
            | StrategyConfig::BbrkRex(_)
            | StrategyConfig::BbrkVol(_)
            | StrategyConfig::BbrkTtm(_)
            | StrategyConfig::BbrkTtmFire(_)
            | StrategyConfig::BbrkCross(_)
            | StrategyConfig::BbrkKdj(_)
            | StrategyConfig::BbrkM2(_)
            | StrategyConfig::BbrkM3(_)
            | StrategyConfig::BbrkOrth(_)
            | StrategyConfig::VwapTrend(_)
            | StrategyConfig::RSI2(_)
            | StrategyConfig::MabbLong(_)
            | StrategyConfig::MabbShort(_)
            | StrategyConfig::Macd(_)
            | StrategyConfig::Vwap(_)
            | StrategyConfig::Edma(_)
            | StrategyConfig::Atma(_)
            | StrategyConfig::Sgma(_) => unreachable!(),
        }
    };
}

/// 构建全部策略, 并对展开后的参数网格做**唯一性自检**。
///
/// # Errors
/// 任一策略的 `param_str(i)` 在 `0..param_count()` 内出现重复时返回
/// [`AppError::Msg`](crate::error::AppError::Msg), 列出冲突串。
///
/// 背景: parquet 的 `strategy_params` 整数列由 `strategy_params_value` 把参数元组
/// 压缩而成, 超出槽位时回退 `fnv1a_32`(**有损**: 不同参数组可映射到同一 u32)。
/// 行主键实为 `(symbol, timeframe, strategy, params)` 四元组, 故该列只是"同策略内
/// 按参数大小排序"的代理, 碰撞不构成正确性灾难; 但它仍会让排序结果错乱。
/// 单测只锁了当前网格(如 BBRK_VOL 720 串), 用户一旦扩大范围就可能引入碰撞且
/// **完全静默**。这里把静默损坏变成构建期硬失败 —— 代价是 O(N) 次 HashSet 插入
/// (N ≤ 数万, 微秒级), 只在每次启动时跑一次。
pub fn build_strategies_checked(
    configs: &[StrategyConfig],
) -> crate::error::Result<Vec<Box<dyn Strategy>>> {
    let strategies = build_strategies(configs);
    check_param_uniqueness(&strategies)?;
    Ok(strategies)
}

/// 逐策略检查 `param_str` 在参数网格内两两不同。
fn check_param_uniqueness(strategies: &[Box<dyn Strategy>]) -> crate::error::Result<()> {
    for st in strategies {
        let n = st.param_count();
        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(n);
        let mut dup: Option<String> = None;
        for i in 0..n {
            let p = st.param_str(i);
            if !seen.insert(p.clone()) && dup.is_none() {
                dup = Some(p);
            }
        }
        if let Some(d) = dup {
            return Err(crate::error::AppError::Msg(format!(
                "[{}] 参数串重复: {d:?} —— 参数网格展开有误(或该策略的 param_str 未覆盖全部维度), \
                 会导致 parquet 的 strategy_params 列碰撞与选参结果错乱",
                st.name()
            )));
        }
    }
    Ok(())
}

pub fn build_strategies(configs: &[StrategyConfig]) -> Vec<Box<dyn Strategy>> {
    let ma_strategies: Vec<Box<dyn Strategy>> = configs
        .iter()
        .filter(|sc| {
            !matches!(
                sc,
                StrategyConfig::Ssf(_)
                    | StrategyConfig::SsfCd(_)
                    | StrategyConfig::SsfSt(_)
                    | StrategyConfig::SsfStCd(_)
                    | StrategyConfig::SmaCd(_)
                    | StrategyConfig::EmaCross(_)
                    | StrategyConfig::Turtle(_)
                    | StrategyConfig::DualThrust(_)
                    | StrategyConfig::RBreaker(_)
                    | StrategyConfig::Fiali(_)
                    | StrategyConfig::Edma(_)
                    | StrategyConfig::Atma(_)
                    | StrategyConfig::Sgma(_)
                    | StrategyConfig::Aberration(_)
                    | StrategyConfig::ATRBand(_)
                    | StrategyConfig::Keltner(_)
                    | StrategyConfig::BBMR(_)
                    | StrategyConfig::Bbrk(_)
                    | StrategyConfig::BbrkAdx(_)
                    | StrategyConfig::BbrkTrail(_)
                    | StrategyConfig::BbrkDon(_)
                    | StrategyConfig::BbrkEma(_)
                    | StrategyConfig::BbrkPctb(_)
                    | StrategyConfig::BbrkAtr(_)
                    | StrategyConfig::BbrkStr(_)
                    | StrategyConfig::BbrkSma(_)
                    | StrategyConfig::BbrkHma(_)
                    | StrategyConfig::BbrkMax(_)
                    | StrategyConfig::BbrkBb(_)
                    | StrategyConfig::BbrkVolEma(_)
                    | StrategyConfig::BbrkAtrEma(_)
                    | StrategyConfig::BbrkSmaEma(_)
                    | StrategyConfig::BbrkHmaEma(_)
                    | StrategyConfig::BbrkHmaV1(_)
                    | StrategyConfig::BbrkHmaV1Sl(_)
                    | StrategyConfig::BbrkHmaV2(_)
                    | StrategyConfig::BbrkHmaV2Sl(_)
                    | StrategyConfig::BbrkHmaV3(_)
                    | StrategyConfig::BbrkHmaV3Sl(_)
                    | StrategyConfig::BbrkHmaV4(_)
                    | StrategyConfig::BbrkHmaV4Sl(_)
                    | StrategyConfig::BbrkHmaV5(_)
                    | StrategyConfig::BbrkHmaV5Sl(_)
                    | StrategyConfig::BbrkZlemaEma(_)
                    | StrategyConfig::BbrkRex(_)
                    | StrategyConfig::BbrkVol(_)
                    | StrategyConfig::BbrkTtm(_)
                    | StrategyConfig::BbrkTtmFire(_)
                    | StrategyConfig::BbrkCross(_)
                    | StrategyConfig::BbrkKdj(_)
                    | StrategyConfig::BbrkM2(_)
                    | StrategyConfig::BbrkM3(_)
                    | StrategyConfig::BbrkOrth(_)
                    | StrategyConfig::VwapTrend(_)
                    | StrategyConfig::RSI2(_)
                    | StrategyConfig::MabbLong(_)
                    | StrategyConfig::MabbShort(_)
                    | StrategyConfig::Macd(_)
                    | StrategyConfig::Vwap(_)
            )
        })
        .flat_map(|sc| {
            build_ma!(
                sc,
                Alma => AlmaStrategy,
                Dema => DemaStrategy,
                Dma => DmaStrategy,
                Ehma => EhmaStrategy,
                Frama => FramaStrategy,
                Smma => SmmaStrategy,
                Ema => EmaStrategy,
                Fwma => FwmaStrategy,
                Hma => HmaStrategy,
                Jma => JmaStrategy,
                Kama => KamaStrategy,
                LinReg => LinRegStrategy,
                McGinley => McGinleyStrategy,
                Pwma => PwmaStrategy,
                SinWma => SinWmaStrategy,
                Sma => SmaStrategy,
                Swma => SwmaStrategy,
                Tema => TemaStrategy,
                Tilson => TilsonStrategy,
                Trima => TrimaStrategy,
                Vidya => VidyaStrategy,
                Vwma => VwmaStrategy,
                Wilders => WildersStrategy,
                Wma => WmaStrategy,
                Zlma => ZlemaStrategy
            )
        })
        .collect();

    let ssf_strategies: Vec<Box<dyn Strategy>> = configs
        .iter()
        .filter_map(|sc| match sc {
            StrategyConfig::Ssf(c) => {
                Some(Box::new(SsfStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::SsfCd(c) => {
                Some(Box::new(SsfCdStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::SmaCd(c) => {
                Some(Box::new(SmaCdStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::SsfSt(c) => {
                Some(Box::new(SsfStStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::SsfStCd(c) => {
                Some(Box::new(SsfStCdStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::EmaCross(c) => {
                Some(Box::new(EmaCrossStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Turtle(c) => {
                Some(Box::new(TurtleStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::DualThrust(c) => {
                Some(Box::new(DualThrustStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::RBreaker(c) => {
                Some(Box::new(RBreakerStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Fiali(c) => {
                Some(Box::new(FialiStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Edma(c) => {
                Some(Box::new(EdmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Atma(c) => {
                Some(Box::new(AtmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Sgma(c) => {
                Some(Box::new(SgmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Aberration(c) => {
                Some(Box::new(AberrationStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::ATRBand(c) => {
                Some(Box::new(ATRBandStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Keltner(c) => {
                Some(Box::new(KeltnerStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BBMR(c) => {
                Some(Box::new(BBMRStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Bbrk(c) => {
                Some(Box::new(BBRKStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkAdx(c) => {
                Some(Box::new(BbrkAdxStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkTrail(c) => {
                Some(Box::new(BbrkTrailStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkDon(c) => {
                Some(Box::new(BbrkDonStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkEma(c) => {
                Some(Box::new(BbrkEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkPctb(c) => {
                Some(Box::new(BbrkPctbStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkAtr(c) => {
                Some(Box::new(BbrkAtrStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkStr(c) => {
                Some(Box::new(BbrkStrStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkSma(c) => {
                Some(Box::new(BbrkSmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHma(c) => {
                Some(Box::new(BbrkHmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkMax(c) => {
                Some(Box::new(BbrkMaxStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkBb(c) => {
                Some(Box::new(BbrkBbStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkVolEma(c) => {
                Some(Box::new(BbrkVolEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkAtrEma(c) => {
                Some(Box::new(BbrkAtrEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkSmaEma(c) => {
                Some(Box::new(BbrkSmaEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaEma(c) => {
                Some(Box::new(BbrkHmaEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV1(c) => {
                Some(Box::new(BbrkHmaV1Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV2(c) => {
                Some(Box::new(BbrkHmaV2Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV3(c) => {
                Some(Box::new(BbrkHmaV3Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV4(c) => {
                Some(Box::new(BbrkHmaV4Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV5(c) => {
                Some(Box::new(BbrkHmaV5Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV1Sl(c) => {
                Some(Box::new(BbrkHmaV1SlStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV2Sl(c) => {
                Some(Box::new(BbrkHmaV2SlStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV3Sl(c) => {
                Some(Box::new(BbrkHmaV3SlStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV4Sl(c) => {
                Some(Box::new(BbrkHmaV4SlStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkHmaV5Sl(c) => {
                Some(Box::new(BbrkHmaV5SlStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkZlemaEma(c) => {
                Some(Box::new(BbrkZlemaEmaStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkRex(c) => {
                Some(Box::new(BbrkRexStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkVol(c) => {
                Some(Box::new(BbrkVolStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkTtm(c) => {
                Some(Box::new(BbrkTtmStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkTtmFire(c) => {
                Some(Box::new(BbrkTtmFireStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkCross(c) => {
                Some(Box::new(BbrkCrossStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkKdj(c) => {
                Some(Box::new(BbrkKdjStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkM2(c) => {
                Some(Box::new(BbrkM2Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkM3(c) => {
                Some(Box::new(BbrkM3Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkOrth(c) => {
                Some(Box::new(BbrkOrthStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::VwapTrend(c) => {
                Some(Box::new(VwapTrendStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::RSI2(c) => {
                Some(Box::new(RSI2Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::MabbLong(c) => {
                Some(Box::new(MabbLongStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::MabbShort(c) => {
                Some(Box::new(MabbShortStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Macd(c) => {
                Some(Box::new(MacdStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::Vwap(c) => {
                Some(Box::new(VwapStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            _ => None,
        })
        .collect();

    ma_strategies.into_iter().chain(ssf_strategies).collect()
}
