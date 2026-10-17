use crate::error::AppError;
use serde::Deserialize;

use crate::config::F64Param;

/// Bollinger Method III 正统形态（W 底/M 顶原文口径，2026-10-11）：
/// BB(20, sma, σ) 经典口径；W 有效 ⇔ 回看窗内先一根 %b<0（轨外）、
/// 后一根 %b 落回带内 [0,1]（原文：“对第二顶相对第一顶的位置没有要求，
/// 只看相对带的位置”），且 MFI(10) 确认（第二处不创新低）；
/// M 顶镜像。形态回看窗与 MFI 周期预注册固定（见 strategy.rs），
/// σ{2.0,2.5,3.0} = 3组+BBRK锚点。
/// 注：`bbrk_orth`（v1，纯价格版）封存不动，本策略为忠实原文的新策略。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BbrkM3Config {
    pub sigma: F64Param,
}

impl BbrkM3Config {
    pub fn values(&self) -> Vec<f64> {
        self.sigma.values()
    }
}

impl BbrkM3Config {
    /// 语义校验(原集中在 `config.rs::Config::validate` 的 match 中,
    /// 已下沉到本类型; 分派处只保留一行调用)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.sigma.validate_range("bbrk_m3.sigma", 0.1, 10.0)?;
        if self.values().is_empty() {
            return Err(AppError::Msg("bbrk_m3 参数组合为空".into()));
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
        let c = BbrkM3Config {
            sigma: F64Param::Range(RangeParam {
                start: 2.0,
                end: 3.0,
                step: 0.5,
            }),
        };
        assert_eq!(c.values(), vec![2.0, 2.5, 3.0]);
    }
}
