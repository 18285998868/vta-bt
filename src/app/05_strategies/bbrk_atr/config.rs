use crate::error::AppError;
use serde::Deserialize;

use crate::config::{F64Param, PeriodParam};

/// BBRK+ATR扩张门（入场验证#4）：进场仍 BB突破 + 方向一致，
/// 叠加 `ATR(atr1)/ATR(atr2) > thr`（Wilder口径）。
/// thr{0.8,1.0,1.2}：1.0为扩张/收缩天然分界。σ × len × atr1 × atr2 × thr 笛卡尔积，
/// 只取 atr1<atr2（短长语义，ema_cross同口径）。
/// BB周期bblen（2026-10-12起参数化；缺省20＝旧串语义）。
/// ATR短长窗（缺省5/20＝旧串语义，经典短长配）。
/// 离场保持中线。逐笔统计预判：thr剂量单调（0.8<1.0<1.2）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkAtrConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: PeriodParam,
    /// ATR短窗（缺省5＝旧串语义）。
    #[serde(default = "default_atr1")]
    /// 快 ATR 周期。
    pub atr1: PeriodParam,
    /// ATR长窗（缺省20＝旧串语义）。
    #[serde(default = "default_atr2")]
    /// 慢 ATR 周期。
    pub atr2: PeriodParam,
    /// ATR 突破阈值(倍数)。
    pub atrthr: F64Param,
}

fn default_bblen() -> PeriodParam {
    PeriodParam::Single(20)
}

fn default_atr1() -> PeriodParam {
    PeriodParam::Single(5)
}

fn default_atr2() -> PeriodParam {
    PeriodParam::Single(20)
}

impl BbrkAtrConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(f64, usize, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let a1 = self.atr1.values();
        let a2 = self.atr2.values();
        let ts = self.atrthr.values();
        // 网格规模 = 各维度长度之积(可预知), 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处
        // (笛卡尔积理论上无界, 普通 `*` 在 debug 下会溢出 panic)。
        let cap =
            crate::strategies::grid_prealloc(&[ss.len(), ls.len(), a1.len(), a2.len(), ts.len()]);
        let mut out = Vec::with_capacity(cap);
        for &s in &ss {
            for &l in &ls {
                for &x in &a1 {
                    for &y in &a2 {
                        let (xi, yi) = (x, y);
                        if xi >= yi {
                            continue;
                        }
                        for &t in &ts {
                            out.push((s, l, xi, yi, t));
                        }
                    }
                }
            }
        }
        out
    }
}

impl BbrkAtrConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_atr.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_atr.bblen", 2, 2000)?;
        self.atr1.validate_range("bbrk_atr.atr1", 2, 500)?;
        self.atr2.validate_range("bbrk_atr.atr2", 2, 500)?;
        self.atrthr.validate_range("bbrk_atr.atrthr", 0.1, 5.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_atr 参数组合为空".into()));
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
    fn missing_bblen_atr_defaults_to_20_5_20() {
        let c: BbrkAtrConfig = toml::from_str("sigma = 2.5\natrthr = 1.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 5, 20, 1.0)]);
    }

    #[test]
    fn short_not_less_than_long_filtered() {
        let c: BbrkAtrConfig =
            toml::from_str("sigma = 2.5\nbblen = 20\natr1 = 10\natr2 = 10\natrthr = 1.0").unwrap();
        assert!(c.pairs().is_empty());
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkAtrConfig {
            sigma: F64Param::Single(2.5),
            bblen: PeriodParam::Single(20),
            atr1: PeriodParam::Single(5),
            atr2: PeriodParam::Single(20),
            atrthr: F64Param::Range(RangeParam {
                start: 0.8,
                end: 1.0,
                step: 0.2,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(2.5, 20, 5, 20, 0.8), (2.5, 20, 5, 20, 1.0)]
        );
    }
}
