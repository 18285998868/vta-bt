use serde::Deserialize;

use crate::config::PeriodParam;

/// SGMA: Savitzky-Golay 平滑（官方 `sgf` endpoint smoother）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SgmaConfig {
    /// 指标周期(bar 数)。
    pub period: PeriodParam,
    #[serde(default)]
    /// 拟合多项式阶数。
    pub poly_order: Option<PeriodParam>,
}

impl SgmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let rp = self.period.to_range();
        let periods: Vec<usize> = (rp.start..=rp.end).step_by(rp.step).collect();
        let orders: Vec<usize> = match &self.poly_order {
            None => vec![2],
            Some(o) => {
                let r = o.to_range();
                (r.start..=r.end).step_by(r.step).collect()
            }
        };
        let mut out = Vec::new();
        for &p in &periods {
            for &o in &orders {
                out.push((p, o));
            }
        }
        out
    }
}

impl SgmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()?;
        Ok(())
    }
}
