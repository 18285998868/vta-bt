use crate::config::{F64Param, PeriodParam};
use crate::error::AppError;
use serde::Deserialize;

/// ATR 平滑方式。
///
/// 为什么用枚举而不是裸 `String`: 裸字符串会同时带来三个问题 ——
/// 1. **类型安全**: 任何拼写错误(`"ewma"`)都能通过 TOML 反序列化, 只能在
///    `validate()` 里被拦, 且拦的是"字符串是否在白名单里";
/// 2. **热路径分配**: `Params` 元组里塞一个 `String`, 意味着每次参数求值都要
///    深拷贝一份堆字符串;
/// 3. **静默兜底**: `atr_by_kind` 的 `match` 写过 `_ => sma_atr` ——
///    未知取值**静默退回**SMA, 报告里的口径与配置声明的不一致却无任何提示。
///
/// 枚举让非法取值在**反序列化期**就不可表示, 且 `Params` 得以 `Copy`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AtrKind {
    /// 简单移动平均(与因子研究侧口径一致)。
    #[default]
    Sma,
    /// Wilder RMA(TradingView / 通达信等平台的默认定义)。
    Wilder,
}

impl AtrKind {
    /// 参数串与错误消息用的文本。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sma => "sma",
            Self::Wilder => "wilder",
        }
    }
}

impl std::fmt::Display for AtrKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 均线斜率 / ATR 波动尺度 门控策略配置。
///
/// 规则(只做多, 不持空):
/// `close > SMA(ma)` **且** `zscore(slope / (ATR/close), z_win) > th` 时持多, 否则空仓。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaSlopeAtrGateConfig {
    /// 均线周期。
    pub ma: PeriodParam,
    /// 均线斜率回看窗口(与 `ma` 同周期均线的相对变化率)。
    pub slope_k: PeriodParam,
    /// ATR 周期(真实波幅均值, 作为波动尺度分母)。
    pub atr_n: PeriodParam,
    /// ATR 平滑方式: `"sma"`(简单移动平均, 与因子研究侧一致)或
    /// `"wilder"`(RMA, TradingView/通达信等平台的默认定义)。
    #[serde(default)]
    pub atr_kind: AtrKind,
    /// 因子的滚动标准化窗口。
    pub z_win: PeriodParam,
    /// 门控阈值(标准化后的下限)。
    pub th: F64Param,
}

/// 单组参数 `(ma, slope_k, atr_n, z_win, th, atr_kind)`。
pub type Params = (usize, usize, usize, usize, f64, AtrKind);

impl MaSlopeAtrGateConfig {
    /// 展开后的参数组合 `(ma, slope_k, atr_n, z_win, th, atr_kind)`。
    #[must_use]
    pub fn quints(&self) -> Vec<Params> {
        let ma_v = self.ma.values();
        let k_v = self.slope_k.values();
        let a_v = self.atr_n.values();
        let z_v = self.z_win.values();
        let t_v = self.th.values();
        // 笛卡尔积长度可预知, 一次分配到位(`mem-with-capacity`)。
        // 乘积的 `saturating_mul` 与封顶逻辑集中在 `grid_prealloc` 一处: 本策略
        // 有**五**个维度, 每维可展开到 `MAX_F64_RANGE` = 10 万档, 直接按积申请
        // 会触发分配失败 → 进程 abort(不可捕获), 而不是返回一条可读错误。
        let cap = crate::strategies::grid_prealloc(&[
            ma_v.len(),
            k_v.len(),
            a_v.len(),
            z_v.len(),
            t_v.len(),
        ]);
        let mut out = Vec::with_capacity(cap);
        for &ma in &ma_v {
            for &k in &k_v {
                for &a in &a_v {
                    for &z in &z_v {
                        for &t in &t_v {
                            out.push((ma, k, a, z, t, self.atr_kind));
                        }
                    }
                }
            }
        }
        out
    }

    /// 语义校验: 参数范围与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界、`slope_k >= ma`(斜率窗口长于均线, 均线尚无值)或展开为空时
    /// 返回 `AppError`(crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.ma.validate_range("ma_slope_atr_gate.ma", 2, 2000)?;
        self.slope_k
            .validate_range("ma_slope_atr_gate.slope_k", 1, 2000)?;
        self.atr_n
            .validate_range("ma_slope_atr_gate.atr_n", 2, 500)?;
        self.z_win
            .validate_range("ma_slope_atr_gate.z_win", 2, 2000)?;
        self.th
            .validate_range("ma_slope_atr_gate.th", -10.0, 10.0)?;
        // 网格只展开**一次**并复用: 原实现调了两次 `quints()`,
        // 等于把整张笛卡尔积算两遍(`validate` 是每次启动都要走的路径)。
        let quints = self.quints();
        if quints.is_empty() {
            return Err(AppError::Msg("ma_slope_atr_gate 参数组合为空".into()));
        }
        for &(ma, k, _, _, _, _) in &quints {
            if k >= ma {
                return Err(AppError::Msg(format!(
                    "ma_slope_atr_gate.slope_k({k}) 必须小于 ma({ma})"
                )));
            }
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MaSlopeAtrGateConfig {
        MaSlopeAtrGateConfig {
            ma: PeriodParam::Single(100),
            slope_k: PeriodParam::Single(20),
            atr_n: PeriodParam::Single(120),
            z_win: PeriodParam::Single(60),
            th: F64Param::Single(0.5),
            atr_kind: AtrKind::Sma,
        }
    }

    #[test]
    fn quints_single() {
        assert_eq!(cfg().quints(), vec![(100, 20, 120, 60, 0.5, AtrKind::Sma)]);
    }

    #[test]
    fn quints_cartesian() {
        let c = MaSlopeAtrGateConfig {
            th: F64Param::Range(crate::config::RangeParam {
                start: 0.25,
                end: 0.75,
                step: 0.25,
            }),
            ..cfg()
        };
        assert_eq!(c.quints().len(), 3);
        assert_eq!(c.quints()[0].4, 0.25);
        assert_eq!(c.quints()[0].5, AtrKind::Sma);
    }

    #[test]
    fn validate_rejects_slope_k_ge_ma() {
        let c = MaSlopeAtrGateConfig {
            ma: PeriodParam::Single(20),
            slope_k: PeriodParam::Single(20),
            ..cfg()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_accepts_defaults() {
        assert!(cfg().validate().is_ok());
    }

    /// 回归: 未知的 `atr_kind` 必须在**反序列化期**就被拒。
    ///
    /// 可触发场景: 原实现用 `String` 承载 `atr_kind`, 拼错的取值能通过反序列化,
    /// 只在 `validate()` 里被字符串白名单拦下; 而 `atr_by_kind` 的 `match` 还有
    /// `_ => sma_atr` 的**静默兜底** —— 一旦某条路径绕过 validate, 口径就悄悄
    /// 变成了 SMA 而报告里看不出任何差别。
    #[test]
    fn unknown_atr_kind_is_rejected_at_deserialization() {
        let toml = r#"
ma = 100
slope_k = 20
atr_n = 14
z_win = 60
th = 0.5
atr_kind = "ewma"
"#;
        let e = toml::from_str::<MaSlopeAtrGateConfig>(toml).expect_err("未知 atr_kind 必须被拒");
        assert!(
            format!("{e}").contains("ewma"),
            "错误消息应指明非法取值: {e}"
        );
    }

    /// 回归: 合法取值必须能反序列化, 且缺省为 SMA。
    #[test]
    fn atr_kind_deserializes_and_defaults_to_sma() {
        let base = "ma = 100\nslope_k = 20\natr_n = 14\nz_win = 60\nth = 0.5\n";
        let d: MaSlopeAtrGateConfig = toml::from_str(base).expect("缺省应可反序列化");
        assert_eq!(d.atr_kind, AtrKind::Sma);
        let w: MaSlopeAtrGateConfig =
            toml::from_str(&format!("{base}atr_kind = \"wilder\"\n")).expect("wilder 应可反序列化");
        assert_eq!(w.atr_kind, AtrKind::Wilder);
    }
}
