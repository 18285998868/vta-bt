use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+K线强度过滤（入场验证#1）：进场仍 BB20σ突破，叠加当根K线
/// 实体占比 `|close−open|/(high−low) >= thr` 且方向一致
/// （多头要求 close>open，空头 close<open）。
/// 与已证伪的方向过滤（SMA_CD，恒等变换）同源可比：若保留率>95%
/// 即第三次恒等陷阱，直接判死。σ × thr 笛卡尔积，thr 取 {0.3,0.5,0.7}。
/// 离场保持中线（隔离进场效应）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkStrConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
    /// K 线实体比例阈值。
    pub body: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkStrConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, f64)> {
        let ss = self.sigma.values();
        let bs = self.bblen.values();
        let ts = self.body.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[ss.len(), bs.len(), ts.len()]);
        let mut out = Vec::with_capacity(cap);
        for &s in &ss {
            for &b in &bs {
                for &t in &ts {
                    out.push((s, b, t));
                }
            }
        }
        out
    }
}

impl BbrkStrConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_str.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_str.bblen", 2, 2000)?;
        self.body.validate_range("bbrk_str.body", 0.0, 1.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_str 参数组合为空".into()));
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
        let c = BbrkStrConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 2.5,
                step: 0.5,
            }),
            body: F64Param::Range(RangeParam {
                start: 0.3,
                end: 0.5,
                step: 0.2,
            }),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(
            c.pairs(),
            vec![
                (2.0, 20, 0.3),
                (2.0, 20, 0.5),
                (2.5, 20, 0.3),
                (2.5, 20, 0.5)
            ]
        );
    }
}
