//! 货币金额: **定点十进制**, 不用 `f64` 承载。
//!
//! # 为什么不用 `f64`
//!
//! `domain-fintech` 的硬规则: "RULE: Never use `f64` for money"。
//! `f64` 只有 53 位有效数字(约 15~17 位十进制), 货币金额在**大额 + 多次累加**
//! 时会累积舍入误差; 更糟的是它**能表达** `NaN` / `Inf` —— 金额为 `NaN` 时
//! 不会被任何比较运算捕获, 会一路污染权益曲线与全部指标。
//!
//! `Decimal` 是 128 位定点数(最多 28 位十进制有效数字), 金额运算**精确**,
//! 且**无法**表示 `NaN` / `Inf`。
//!
//! # 边界: 什么**不是**金额
//!
//! - **价格**(open/high/low/close) 仍是 `f64`: 价格是连续量, 且要参与
//!   `ln` / `sqrt` 等超越函数(指标与统计口径), 定点化没有意义。
//! - **比率**(fee_rate / slip_rate / pnl_pct) 仍是 `f64`: 无量纲。
//! - **权益曲线**(`equity_curve` / `fixed_curve`) 仍是 `f64`: 它是**序列**,
//!   供 `CurveAccum` 算对数收益与 PBO 的 CSCV 用, 需要超越函数。
//!
//! 金额只出现在: 资金余额、持仓数量、单笔盈亏、累计已实现盈亏。

use rust_decimal::Decimal;
use rust_decimal::prelude::*;

/// 金额(货币)。定点十进制, **不是** `f64`。
///
/// 不可变 + `Copy`(内部是 128 位整数 + 标度), 算术**全部返回 `Result`** ——
/// 溢出、除零、精度截断都会显式报错, 而不是静默回绕或产出 `Inf`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Amount(Decimal);

/// `f64` → `Decimal`(丢弃 f64 的"多余位", 0.1 → 0.1)。
fn decimal_ratio(v: f64) -> crate::error::Result<Decimal> {
    if !v.is_finite() {
        return Err(crate::error::AppError::Msg(format!("非法比率: {v}")));
    }
    Decimal::from_f64(v)
        .ok_or_else(|| crate::error::AppError::Msg(format!("比率无法转成十进制: {v}")))
}

impl Amount {
    /// 零。
    pub const ZERO: Amount = Amount(Decimal::ZERO);

    /// 由整数构造。
    #[must_use]
    pub fn from_i64(v: i64) -> Amount {
        Amount(Decimal::from(v))
    }

    /// 由 `Decimal` 构造(不损失精度)。
    #[must_use]
    pub fn from_decimal(d: Decimal) -> Amount {
        Amount(d)
    }

    /// 由 `f64` 构造。
    ///
    /// `f64` → `Decimal` 会按 `Decimal::from_f64` 的语义取整到 15 位有效数字
    /// (而不是精确二进制展开) —— 对"从配置里的 `initial_balance = 10000.0`
    /// 读进来的钱"这正是想要的: **用户写的十进制**被原样保留。
    ///
    /// # Errors
    /// `v` 非有限(`NaN` / `Inf`)时返回 [`AppError`](crate::error::AppError) ——
    /// 金额为 `NaN` 是**不可接受**的, 必须显式拒绝。
    pub fn from_f64(v: f64) -> crate::error::Result<Amount> {
        if !v.is_finite() {
            return Err(crate::error::AppError::Msg(format!(
                "金额不能是 NaN/Inf: {v}"
            )));
        }
        // `Decimal::from_f64` 丢弃 f64 的"多余位"(0.1 → 0.1), 这正是
        // "用户写的十进制被原样保留"的语义; 它对非有限值返回 `None`。
        Decimal::from_f64(v)
            .map(Amount)
            .ok_or_else(|| crate::error::AppError::Msg(format!("金额不能是 NaN/Inf: {v}")))
    }

    /// 底层 `Decimal`。
    #[must_use]
    pub fn decimal(self) -> Decimal {
        self.0
    }

    /// 转回 `f64`(仅用于**落盘**与统计口径; 内部金额运算不得用)。
    #[must_use]
    pub fn to_f64(self) -> f64 {
        self.0.to_f64().unwrap_or(f64::NAN)
    }

    /// 是否为零。
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// 取相反数。
    #[must_use]
    pub fn negated(self) -> Amount {
        Amount(-self.0)
    }

    /// 加法。
    ///
    /// # Errors
    /// 溢出(超出 `Decimal` 的 28 位有效数字)时返回 [`AppError`](crate::error::AppError)。
    pub fn plus(self, other: Amount) -> crate::error::Result<Amount> {
        self.0.checked_add(other.0).map(Amount).ok_or_else(|| {
            crate::error::AppError::Msg(format!("金额加法溢出: {} + {}", self.0, other.0))
        })
    }

    /// 减法。
    ///
    /// # Errors
    /// 溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn minus(self, other: Amount) -> crate::error::Result<Amount> {
        self.0.checked_sub(other.0).map(Amount).ok_or_else(|| {
            crate::error::AppError::Msg(format!("金额减法溢出: {} - {}", self.0, other.0))
        })
    }

    /// 乘以一个**无量纲**比率(费率、滑点率)。
    ///
    /// # Errors
    /// 比率非有限或结果溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn mul_ratio(self, ratio: f64) -> crate::error::Result<Amount> {
        if !ratio.is_finite() {
            return Err(crate::error::AppError::Msg(format!(
                "金额乘以非法比率: {ratio}"
            )));
        }
        self.0
            .checked_mul(decimal_ratio(ratio)?)
            .map(Amount)
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("金额乘法溢出: {} * {ratio}", self.0))
            })
    }

    /// 乘以一个**价格**(得到名义金额)。
    ///
    /// # Errors
    /// 价格非有限或结果溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn mul_price(self, price: f64) -> crate::error::Result<Amount> {
        if !price.is_finite() {
            return Err(crate::error::AppError::Msg(format!(
                "金额乘以非法价格: {price}"
            )));
        }
        self.0
            .checked_mul(decimal_ratio(price)?)
            .map(Amount)
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("金额乘法溢出: {} * {price}", self.0))
            })
    }

    /// 除以一个**价格**(得到持仓数量 —— 也是金额量纲)。
    ///
    /// # Errors
    /// 价格为 0 / 非有限, 或结果溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn div_price(self, price: f64) -> crate::error::Result<Amount> {
        if !price.is_finite() || price == 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "金额除以非法价格: {price}"
            )));
        }
        self.0
            .checked_div(decimal_ratio(price)?)
            .map(Amount)
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("金额除法溢出: {} / {price}", self.0))
            })
    }

    /// 除以一个**无量纲**比率。
    ///
    /// # Errors
    /// 比率为 0 / 非有限, 或结果溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn div_ratio(self, ratio: f64) -> crate::error::Result<Amount> {
        if !ratio.is_finite() || ratio == 0.0 {
            return Err(crate::error::AppError::Msg(format!(
                "金额除以非法比率: {ratio}"
            )));
        }
        self.0
            .checked_div(decimal_ratio(ratio)?)
            .map(Amount)
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("金额除法溢出: {} / {ratio}", self.0))
            })
    }

    /// 绝对值。
    #[must_use]
    pub fn abs(self) -> Amount {
        Amount(self.0.abs())
    }

    /// 除以一个**笔数**(得到平均金额)。
    ///
    /// # Errors
    /// 笔数为 0, 或结果溢出时返回 [`AppError`](crate::error::AppError)。
    pub fn div_count(self, count: i64) -> crate::error::Result<Amount> {
        if count == 0 {
            return Err(crate::error::AppError::Msg(
                "金额除以 0 笔(除零)".to_string(),
            ));
        }
        self.0
            .checked_div(Decimal::from(count))
            .map(Amount)
            .ok_or_else(|| {
                crate::error::AppError::Msg(format!("金额除法溢出: {} / {count}", self.0))
            })
    }

    /// 按**标度**重标定(用于 Parquet `Decimal128` 的定标存储)。
    ///
    /// 降标度时会**四舍五入**(中点远离零); 升标度只补零, 不损失精度。
    /// 故本方法**不会失败** —— 需要"必须无损"的场景请用
    /// [`Self::to_decimal128_mantissa`], 它会在需要舍入时显式报错。
    #[must_use]
    pub fn rescale(self, scale: u32) -> Amount {
        let mut d = self.0;
        d.rescale(scale);
        Amount(d)
    }

    /// 转成 Parquet `Decimal128` 的尾数(定点整数表示)。
    ///
    /// `Decimal128(38, scale)` 这个**类型**就只能表示 `scale` 位小数 ——
    /// 超过的部分**必然**要舍入(中点远离零)。故本方法做的是"按列的标度舍入",
    /// 不是"丢弃精度": 金额落到固定标度上是**类型契约**, 不是缺陷。
    ///
    /// 之所以仍返回 `Result`: 标度非法(> 28)时报错, 而不是产出垃圾尾数。
    pub fn to_decimal128_mantissa(self, scale: u32) -> crate::error::Result<i128> {
        if scale > rust_decimal::Decimal::MAX_SCALE {
            return Err(crate::error::AppError::Msg(format!(
                "金额标度 {scale} 超过 Decimal 的上限 {}",
                rust_decimal::Decimal::MAX_SCALE
            )));
        }
        Ok(self.rescale(scale).0.mantissa())
    }

    /// 由 Parquet `Decimal128` 的尾数构造。
    #[must_use]
    pub fn from_decimal128_mantissa(mantissa: i128, scale: u32) -> Amount {
        Amount(Decimal::from_i128_with_scale(mantissa, scale))
    }
}

impl std::fmt::Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parquet `Decimal128` 金额列的**标度**(小数位数)。
///
/// 选 8 位: 覆盖加密货币常见的 8 位精度(如 `0.00000001`), 同时
/// `Decimal128` 的 38 位总精度里留出 30 位整数位 —— 远大于任何账户余额。
pub const MONEY_SCALE: u32 = 8;

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_arithmetic_is_exact() {
        // f64 在这里会丢精度: 0.1 + 0.2 != 0.3。
        let a = Amount::from_f64(0.1).unwrap();
        let b = Amount::from_f64(0.2).unwrap();
        let c = Amount::from_f64(0.3).unwrap();
        assert_eq!(a.plus(b).unwrap(), c, "十进制加法必须精确");
    }

    #[test]
    fn money_rejects_non_finite() {
        assert!(Amount::from_f64(f64::NAN).is_err());
        assert!(Amount::from_f64(f64::INFINITY).is_err());
        assert!(Amount::from_f64(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn division_by_zero_is_an_error_not_inf() {
        let a = Amount::from_i64(100);
        assert!(a.div_price(0.0).is_err());
        assert!(a.div_ratio(0.0).is_err());
    }

    #[test]
    fn mul_by_non_finite_is_an_error() {
        let a = Amount::from_i64(100);
        assert!(a.mul_ratio(f64::NAN).is_err());
        assert!(a.mul_price(f64::INFINITY).is_err());
    }

    #[test]
    fn rescale_to_money_scale_is_lossless_for_money() {
        let a = Amount::from_f64(1234.5678).unwrap();
        let b = a.rescale(MONEY_SCALE);
        assert_eq!(b.to_f64(), 1234.5678);
    }

    #[test]
    fn decimal128_mantissa_round_trip_is_exact() {
        let a = Amount::from_f64(1234.5678).unwrap();
        let m = a.to_decimal128_mantissa(MONEY_SCALE).expect("8 位标度足够");
        let back = Amount::from_decimal128_mantissa(m, MONEY_SCALE);
        assert_eq!(back, a);
        assert_eq!(back.to_f64(), 1234.5678);
    }

    #[test]
    fn decimal128_mantissa_rounds_to_the_column_scale() {
        // `Decimal128(38, 8)` 这个**类型**只能表示 8 位小数 ⇒ 超出部分按列标度舍入。
        let a = Amount::from_f64(1.234_567_891).unwrap();
        let m = a.to_decimal128_mantissa(MONEY_SCALE).expect("8 位标度合法");
        assert_eq!(m, 123_456_789);
        let back = Amount::from_decimal128_mantissa(m, MONEY_SCALE);
        assert!((back.to_f64() - 1.234_567_89).abs() < 1e-9);
    }

    #[test]
    fn decimal128_mantissa_rejects_an_illegal_scale() {
        assert!(Amount::from_i64(1).to_decimal128_mantissa(99).is_err());
    }

    #[test]
    fn zero_and_negate() {
        assert!(Amount::ZERO.is_zero());
        assert_eq!(Amount::from_i64(5).negated(), Amount::from_i64(-5));
        assert!(Amount::default().is_zero());
    }
}
