use serde::Deserialize;

use crate::config::PeriodParam;

/// EDMA: Exponential Displaced Moving Average。
/// `edma[i] = ema[i - offset]`（向后偏移 = 延迟版 EMA，无前视）。
/// period/offset 独立成范围，笛卡尔扫描。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdmaConfig {
    /// 指标周期(bar 数)。
    pub period: PeriodParam,
    #[serde(default)]
    /// 策略参数 `offset`。
    pub offset: Option<PeriodParam>,
}

impl EdmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let pr = self.period.to_range();
        let periods: Vec<usize> = (pr.start..=pr.end).step_by(pr.step).collect();
        let offsets: Vec<usize> = match &self.offset {
            None => vec![0],
            Some(o) => {
                let r = o.to_range();
                (r.start..=r.end).step_by(r.step).collect()
            }
        };
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[periods.len(), offsets.len()]);
        let mut out = Vec::with_capacity(cap);
        for &p in &periods {
            for &o in &offsets {
                out.push((p, o));
            }
        }
        out
    }
}

impl EdmaConfig {
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    ///
    /// `offset` **必须**一并校验: 它是 `Option<PeriodParam>`, 原实现只校验了
    /// `period`, 于是 `offset = { start = 0, end = 2, step = 0 }` 能通过校验,
    /// 却在 `pairs()` 里触发 `Range::step_by(0)` 的 **panic** ——
    /// 用户 TOML 可直接触发的运行时崩溃。
    ///
    /// 注意 `offset` 的合法下界是 **0**(不偏移是合法配置), 故不能用
    /// `PeriodParam::validate()`(它要求 >= 1), 只能用具名版本。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()?;
        if let Some(o) = &self.offset {
            o.validate_named("edma.offset", 0)?;
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn pairs_cartesian() {
        let c = EdmaConfig {
            period: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            offset: Some(PeriodParam::Range(RangeParam {
                start: 0,
                end: 2,
                step: 2,
            })),
        };
        assert_eq!(c.pairs(), vec![(5, 0), (5, 2), (10, 0), (10, 2)]);
    }

    /// 回归: `offset.step = 0` 必须在**校验期**被拒。
    ///
    /// 可触发场景: 原实现只校验 `period`, `offset` 完全逃过检查, 于是
    /// `pairs()` 里的 `.step_by(0)` panic("assertion failed: step != 0")。
    #[test]
    fn zero_step_in_offset_is_rejected_before_panicking() {
        let c = EdmaConfig {
            period: PeriodParam::Single(10),
            offset: Some(PeriodParam::Range(RangeParam {
                start: 0,
                end: 2,
                step: 0,
            })),
        };
        let e = c.validate().expect_err("step=0 必须被拒");
        assert!(
            format!("{e}").contains("edma.offset"),
            "错误消息应指明是哪个参数: {e}"
        );
    }

    /// 回归: `offset = 0` 是**合法**配置, 不得被 `period` 的下界规则误拒。
    #[test]
    fn zero_offset_is_valid() {
        let c = EdmaConfig {
            period: PeriodParam::Single(10),
            offset: Some(PeriodParam::Single(0)),
        };
        assert!(c.validate().is_ok());
    }
}
