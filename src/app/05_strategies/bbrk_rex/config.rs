use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+振幅扩张过滤（入场验证#2，用户设计）：
/// 当根振幅与过去 n 根对比：`(high−low) > k × mean(high−low, n)`，
/// 且方向一致（多头 close>open），进场仍 BB20σ突破。
/// n 用户指定 {3,5,7,9,11,13,15}；k{1.5,2,2.5,3}（2026-10-12起参数化，
/// 原mult=1.5固定；旧"sigma=S,rexn=N"参数串封存，新串多",rexmult=M"后缀）。
/// BB周期bblen（2026-10-12起参数化，默认20即经典；旧串视为len20）。
/// 与验证#1对照：方向要求相同，差异只在强度定义
/// （根内实体占比 vs 跨根振幅扩张）。离场保持中线。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkRexConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
    /// BB周期（2026-10-12起参数化；缺省20＝旧串语义，封存数据即len20）。
    #[serde(default = "default_bblen")]
    /// 布林带周期。
    pub bblen: F64Param,
    /// 挤压计数窗口。
    pub rexn: F64Param,
    /// 挤压判定倍数。
    pub rexmult: F64Param,
}

fn default_bblen() -> F64Param {
    F64Param::Single(20.0)
}

impl BbrkRexConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    pub fn pairs(&self) -> Vec<(f64, usize, usize, f64)> {
        let ss = self.sigma.values();
        let ls = self.bblen.values();
        let ns = self.rexn.values();
        let ms = self.rexmult.values();
        let mut out = Vec::new();
        for &s in &ss {
            for &l in &ls {
                for &n in &ns {
                    for &m in &ms {
                        out.push((s, l.round() as usize, n.round() as usize, m));
                    }
                }
            }
        }
        out
    }
}

impl BbrkRexConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_rex.sigma", 0.1, 10.0)?;
        self.bblen.validate_range("bbrk_rex.bblen", 2.0, 2000.0)?;
        self.rexn.validate_range("bbrk_rex.rexn", 2.0, 100.0)?;
        self.rexmult.validate_range("bbrk_rex.rexmult", 0.5, 30.0)?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("bbrk_rex 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn missing_bblen_defaults_to_20() {
        let c: BbrkRexConfig = toml::from_str("sigma = 2.5\nrexn = 7.0\nrexmult = 2.0").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 7, 2.0)]);
    }

    #[test]
    fn pairs_cartesian() {
        let c = BbrkRexConfig {
            sigma: F64Param::Single(2.5),
            bblen: F64Param::Single(20.0),
            rexn: F64Param::Range(RangeParam {
                start: 3.0,
                end: 5.0,
                step: 2.0,
            }),
            rexmult: F64Param::Single(2.0),
        };
        assert_eq!(c.pairs(), vec![(2.5, 20, 3, 2.0), (2.5, 20, 5, 2.0)]);
    }
}
