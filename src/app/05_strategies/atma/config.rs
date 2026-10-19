use serde::Deserialize;

use crate::config::PeriodParam;

/// ATMA: up/down 双周期笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtmaConfig {
    /// 上轨周期。
    pub up: PeriodParam,
    /// 下轨周期。
    pub down: PeriodParam,
}

impl AtmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let ru = self.up.to_range();
        let rd = self.down.to_range();
        let ups: Vec<usize> = (ru.start..=ru.end).step_by(ru.step).collect();
        let downs: Vec<usize> = (rd.start..=rd.end).step_by(rd.step).collect();
        let mut out = Vec::new();
        for &u in &ups {
            for &d in &downs {
                out.push((u, d));
            }
        }
        out
    }
}

impl AtmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.up.validate()?;
        self.down.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = AtmaConfig {
            up: PeriodParam::Single(10),
            down: PeriodParam::Range(RangeParam {
                start: 20,
                end: 40,
                step: 20,
            }),
        };
        assert_eq!(c.pairs(), vec![(10, 20), (10, 40)]);
    }
}
