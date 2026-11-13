use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+量能门（入场验证#3，用户设计）：
/// 当根成交量 > m × mean(volume, N)，N{5,10,15,20}，m{1.2,1.5,2,2.5,3}；
/// 进场仍 BB突破 + 方向一致（与STR/REX同口径）。
/// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
/// 离场保持中线。σ × len × N × m 四笛卡尔。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkVolConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// 量能均值窗口(bar 数)。
    pub voln: PeriodParam,
    /// 量能放大倍数阈值。
    pub volmult: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkVolConfig {
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
            for &l in &self.bblen.values() {
                for &n in &self.voln.values() {
                    for &m in &self.volmult.values() {
                        out.push((s, l, n, m));
                    }
                }
            }
        }
        out
    }
}

impl BbrkVolConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_vol.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_vol.bblen", 2, 2000)?;
        self.voln.validate_range("bbrk_vol.voln", 2, 500)?;
        self.volmult.validate_range("bbrk_vol.volmult", 0.5, 30.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_vol 参数组合为空".into()));
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
    fn missing_bblen_defaults_to_20() {
        let c: BbrkVolConfig = toml::from_str("sigma = 2.5\nvoln = 10\nvolmult = 1.5").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10, 1.5)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkVolConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            voln: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
            volmult: F64Param::Single(1.5),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 1.5), (2.5, 20, 10, 1.5)]);
    }
}
