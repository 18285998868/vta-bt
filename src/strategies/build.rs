use super::Strategy;
use super::alma::AlmaStrategy;
use super::dema::DemaStrategy;
use super::ema::EmaStrategy;
use super::fwma::FwmaStrategy;
use super::hma::HmaStrategy;
use super::jma::JmaStrategy;
use super::kama::KamaStrategy;
use super::linreg::LinRegStrategy;
use super::mcginley::McGinleyStrategy;
use super::pwma::PwmaStrategy;
use super::sinwma::SinWmaStrategy;
use super::sma::SmaStrategy;
use super::ssf::SsfStrategy;
use super::ssf_st::SsfStStrategy;
use super::swma::SwmaStrategy;
use super::tema::TemaStrategy;
use super::tilson::TilsonStrategy;
use super::trima::TrimaStrategy;
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
            StrategyConfig::Ssf(_) | StrategyConfig::SsfSt(_) => unreachable!(),
        }
    };
}

pub fn build_strategies(configs: &[StrategyConfig]) -> Vec<Box<dyn Strategy>> {
    let ma_strategies: Vec<Box<dyn Strategy>> = configs
        .iter()
        .filter(|sc| !matches!(sc, StrategyConfig::Ssf(_) | StrategyConfig::SsfSt(_)))
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
            StrategyConfig::SsfSt(c) => {
                Some(Box::new(SsfStStrategy::new(c.clone())) as Box<dyn Strategy>)
            }
            _ => None,
        })
        .collect();

    ma_strategies.into_iter().chain(ssf_strategies).collect()
}
