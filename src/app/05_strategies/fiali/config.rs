use serde::Deserialize;

/// 菲阿里四价：零自由参数(上轨=昨日最高，下轨=昨日最低，开盘价止损，
/// UTC 日界平仓，日内各方向限1次)。单配置 N=1。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FialiConfig {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_parses() {
        let c: FialiConfig = toml::from_str("").unwrap();
        let _ = c;
    }
}
