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
    #[must_use]
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
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[periods.len(), orders.len()]);
        let mut out = Vec::with_capacity(cap);
        for &p in &periods {
            for &o in &orders {
                out.push((p, o));
            }
        }
        out
    }
}

impl SgmaConfig {
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    ///
    /// `poly_order` **必须**一并校验: 它是 `Option<PeriodParam>`, 原实现只校验了
    /// `period`, 于是 `poly_order = { start = 2, end = 4, step = 0 }` 能通过校验,
    /// 却在 `pairs()` 里触发 `Range::step_by(0)` 的 **panic** ——
    /// 用户 TOML 可直接触发的运行时崩溃, 且发生在参数展开这种"本该最安全"的环节。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.period.validate()?;
        if let Some(o) = &self.poly_order {
            // 阶数下界为 0(0 阶在数学上退化但不会 panic; 由 vector-ta 侧返回错误
            // → 策略全 Hold, 不静默产出假信号)。
            o.validate_named("sgma.poly_order", 0)?;
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
        let c = SgmaConfig {
            period: PeriodParam::Single(10),
            poly_order: Some(PeriodParam::Range(RangeParam {
                start: 2,
                end: 4,
                step: 1,
            })),
        };
        assert_eq!(c.pairs(), vec![(10, 2), (10, 3), (10, 4)]);
    }

    /// 回归: `poly_order.step = 0` 必须在**校验期**被拒。
    ///
    /// 可触发场景: 原实现只校验 `period`, `poly_order` 完全逃过检查,
    /// 于是 `pairs()` 里的 `(r.start..=r.end).step_by(r.step)` 在 step=0 时
    /// panic("assertion failed: step != 0") —— 用户 TOML 直接可触发。
    #[test]
    fn zero_step_in_poly_order_is_rejected_before_panicking() {
        let c = SgmaConfig {
            period: PeriodParam::Single(10),
            poly_order: Some(PeriodParam::Range(RangeParam {
                start: 2,
                end: 4,
                step: 0,
            })),
        };
        let e = c.validate().expect_err("step=0 必须被拒");
        assert!(
            format!("{e}").contains("sgma.poly_order"),
            "错误消息应指明是哪个参数: {e}"
        );
        // 校验失败 ⇒ 绝不会走到 pairs() 的 panic。
        assert!(c.validate().is_err());
    }

    /// 回归: `poly_order` 的合法值不得被误拒。
    #[test]
    fn valid_poly_order_passes_validation() {
        let c = SgmaConfig {
            period: PeriodParam::Single(10),
            poly_order: Some(PeriodParam::Single(2)),
        };
        assert!(c.validate().is_ok());
        // 缺省(None)也必须通过。
        let c = SgmaConfig {
            period: PeriodParam::Single(10),
            poly_order: None,
        };
        assert!(c.validate().is_ok());
    }
}
