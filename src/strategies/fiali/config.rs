use serde::Deserialize;

/// 菲阿里四价：零自由参数(上轨=昨日最高，下轨=昨日最低，开盘价止损，
/// UTC 日界平仓，日内各方向限1次)。单配置 N=1。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FialiConfig {}

impl FialiConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_parses() {
        let c: FialiConfig = toml::from_str("").unwrap();
        let _ = c;
    }
}
