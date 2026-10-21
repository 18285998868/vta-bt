use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+量能门（EMA基线版：BB中轨ema，其余与 bbrk_vol 逐位一致）：
/// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
/// 进场仍 BB20σ突破 + 方向一致（与STR/REX同口径）。
/// 离场保持中线。σ × N × m 三笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkVolEmaConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB 周期(缺省 20＝旧串语义: 此前该策略把 BB 周期写死为 20)。
    #[serde(default = "default_bblen")]
    pub bblen: PeriodParam,
    /// 量能均值窗口(bar 数)。
    pub voln: PeriodParam,
    /// 量能放大倍数阈值。
    pub volmult: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkVolEmaConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap = crate::strategies::grid_prealloc(&[
            self.sigma.values().len(),
            self.bblen.values().len(),
            self.voln.values().len(),
            self.volmult.values().len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &s in &self.sigma.values() {
            for &b in &self.bblen.values() {
                for &n in &self.voln.values() {
                    for &m in &self.volmult.values() {
                        out.push((s, b, n, m));
                    }
                }
            }
        }
        out
    }
}

impl BbrkVolEmaConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_vol_ema.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_vol_ema.bblen", 2, 2000)?;
        self.voln.validate_range("bbrk_vol_ema.voln", 2, 500)?;
        self.volmult
            .validate_range("bbrk_vol_ema.volmult", 0.5, 30.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_vol_ema 参数组合为空".into()));
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
        let c = BbrkVolEmaConfig {
            sigma: F64Param::Single(2.5),
            voln: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            volmult: F64Param::Single(1.5),
            bblen: crate::config::PeriodParam::Single(20),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 1.5), (2.5, 20, 10, 1.5)]);
    }
}
