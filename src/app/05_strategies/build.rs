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
use super::bbrk_bw::BbrkBwStrategy;
use super::bbrk_cross::BbrkCrossStrategy;
use super::bbrk_don::BbrkDonStrategy;
use super::bbrk_hma::BbrkHmaStrategy;
use super::bbrk_hma_ema::BbrkHmaEmaStrategy;
use super::bbrk_m2::BbrkM2Strategy;
use super::bbrk_m3::BbrkM3Strategy;
use super::bbrk_max::BbrkMaxStrategy;
use super::bbrk_orth::BbrkOrthStrategy;
use super::bbrk_pctb::BbrkPctbStrategy;
use super::bbrk_rex::BbrkRexStrategy;
use super::bbrk_sma::BbrkSmaStrategy;
use super::bbrk_sma_ema::BbrkSmaEmaStrategy;
use super::bbrk_sqz::BbrkSqzStrategy;
use super::bbrk_str::BbrkStrStrategy;
use super::bbrk_trail::BbrkTrailStrategy;
use super::bbrk_ttm::BbrkTtmStrategy;
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
            | StrategyConfig::BbrkPctb(_)
            | StrategyConfig::BbrkSqz(_)
            | StrategyConfig::BbrkBw(_)
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
            | StrategyConfig::BbrkZlemaEma(_)
            | StrategyConfig::BbrkRex(_)
            | StrategyConfig::BbrkVol(_)
            | StrategyConfig::BbrkTtm(_)
            | StrategyConfig::BbrkCross(_)
            | StrategyConfig::BbrkM2(_)
            | StrategyConfig::BbrkM3(_)
            | StrategyConfig::BbrkOrth(_)
            | StrategyConfig::VwapTrend(_)
            | StrategyConfig::RSI2(_)
            | StrategyConfig::Macd(_)
            | StrategyConfig::Vwap(_)
            | StrategyConfig::Edma(_)
            | StrategyConfig::Atma(_)
            | StrategyConfig::Sgma(_) => unreachable!(),
        }
    };
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
                    | StrategyConfig::BbrkPctb(_)
                    | StrategyConfig::BbrkSqz(_)
                    | StrategyConfig::BbrkBw(_)
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
                    | StrategyConfig::BbrkZlemaEma(_)
                    | StrategyConfig::BbrkRex(_)
                    | StrategyConfig::BbrkVol(_)
                    | StrategyConfig::BbrkTtm(_)
                    | StrategyConfig::BbrkCross(_)
                    | StrategyConfig::BbrkM2(_)
                    | StrategyConfig::BbrkM3(_)
                    | StrategyConfig::BbrkOrth(_)
                    | StrategyConfig::VwapTrend(_)
                    | StrategyConfig::RSI2(_)
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
            StrategyConfig::BbrkPctb(c) => {
                Some(Box::new(BbrkPctbStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkSqz(c) => {
                Some(Box::new(BbrkSqzStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            StrategyConfig::BbrkBw(c) => {
                Some(Box::new(BbrkBwStrategy::new(c.clone())) as Box<dyn Strategy>)
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
            StrategyConfig::BbrkCross(c) => {
                Some(Box::new(BbrkCrossStrategy::new(c.clone())) as Box<dyn Strategy>)
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
