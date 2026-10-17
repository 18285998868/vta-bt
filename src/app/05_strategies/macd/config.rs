use crate::error::AppError;
use serde::Deserialize;

/// MACD信号线穿越：显式三元组列表(避免笛卡尔误伤出fast>=slow非法组合)。
/// 试点锁定标准组(12,26,9)+短周期组(5,35,5)，N=组数。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacdConfig {
    pub groups: Vec<[usize; 3]>,
}

impl MacdConfig {
    pub fn triples(&self) -> Vec<(usize, usize, usize)> {
        self.groups.iter().map(|g| (g[0], g[1], g[2])).collect()
    }
}

impl MacdConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.triples().is_empty() {
            return Err(AppError::Msg("macd 参数组合为空".into()));
        }
        for (f, s, g) in self.triples() {
            if f == 0 || s == 0 || g == 0 || f >= s {
                return Err(AppError::Msg(format!(
                    "macd 非法三元组 ({f},{s},{g})：需 0<fast<slow, signal>0"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_passthrough() {
        let c = MacdConfig {
            groups: vec![[12, 26, 9], [5, 35, 5]],
        };
        assert_eq!(c.triples(), vec![(12, 26, 9), (5, 35, 5)]);
    }
}
