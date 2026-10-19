use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// BBRK+KDJ逆势波段（视频策略改编，独立研究`策略研究记录/KDJ-001/`）：
/// 挤压（BB inside KC，KC=SMA20±1.5·ATR14，视频默认）＋ KDJ(25)超买超卖翻转。
/// 多头：布防（squeeze_on ＋ close≤lower ＋ K<20）→ 触发（K>20 ＋ close>open）
/// → Buy，止损＝触发根前推10根最低；离场：持仓见过K>80后K<80 → Close。
/// 空头镜像。KDJ slow按库默认（映射假设已声明）。
/// σ × 3档；KDJ周期等全固定（一次一变：只测“逆势结构”本身）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkKdjConfig {
    /// 布林带标准差倍数(通道宽度)。
    pub sigma: F64Param,
}

impl BbrkKdjConfig {
    /// 展开后的取值列表。
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

impl BbrkKdjConfig {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_kdj.sigma", 0.1, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("bbrk_kdj 参数组合为空".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RangeParam;

    #[test]
    fn values_expand() {
        let c = BbrkKdjConfig {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
