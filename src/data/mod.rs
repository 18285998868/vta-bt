use std::path::PathBuf;

pub fn load_columns(
    config: &reader::DataConfig,
    symbol: &str,
    timeframe: &str,
) -> anyhow::Result<reader::KlineColumns> {
    let spec = reader::DataSpec {
        source: config.source_type,
        dir: PathBuf::from(&config.dir),
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        start_date: config.start_date.clone(),
        end_date: config.end_date.clone(),
    };
    Ok(reader::load_columns(&spec)?)
}

pub fn load_close(
    config: &reader::DataConfig,
    symbol: &str,
    timeframe: &str,
) -> anyhow::Result<Vec<f64>> {
    let cols = load_columns(config, symbol, timeframe)?;
    Ok(cols.close)
}
