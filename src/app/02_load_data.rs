use std::path::PathBuf;

pub fn load_columns(
    config: &lib_reader::DataConfig,
    symbol: &str,
    timeframe: &str,
) -> crate::error::Result<lib_reader::KlineColumns> {
    let spec = lib_reader::DataSpec {
        source: config.source_type,
        dir: PathBuf::from(&config.dir),
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        start_date: config.start_date.clone(),
        end_date: config.end_date.clone(),
    };
    Ok(lib_reader::load_columns(&spec)?)
}
