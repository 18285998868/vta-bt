use super::Strategy;
use super::aberration::AberrationStrategy;
use super::alma::AlmaStrategy;
use super::atrband::ATRBandStrategy;
use super::bbmr::BBMRStrategy;
use super::dema::DemaStrategy;
use super::dualthrust::DualThrustStrategy;
use super::ema::EmaStrategy;
use super::ema_cross::EmaCrossStrategy;
use super::fiali::FialiStrategy;
use super::fwma::FwmaStrategy;
use super::hma::HmaStrategy;
use super::jma::JmaStrategy;
use super::kama::KamaStrategy;
use super::keltner::KeltnerStrategy;
use super::linreg::LinRegStrategy;
use super::mcginley::McGinleyStrategy;
use super::pwma::PwmaStrategy;
use super::rbreaker::RBreakerStrategy;
use super::rsi2::RSI2Strategy;
use super::sinwma::SinWmaStrategy;
use super::sma::SmaStrategy;
use super::sma_cd::SmaCdStrategy;
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
            | StrategyConfig::RSI2(_) => unreachable!(),
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
                    | StrategyConfig::Aberration(_)
                    | StrategyConfig::ATRBand(_)
                    | StrategyConfig::Keltner(_)
                    | StrategyConfig::BBMR(_)
                    | StrategyConfig::RSI2(_)
            )
        })
        .flat_map(|sc| {
            build_ma!(
                sc,
                Alma => AlmaStrategy,
                Dema => DemaStrategy,
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
            StrategyConfig::RSI2(c) => {
                Some(Box::new(RSI2Strategy::new(c.clone())) as Box<dyn Strategy>)
            }
            _ => None,
        })
        .collect();

    ma_strategies.into_iter().chain(ssf_strategies).collect()
}
