//! 策略信号: 策略与引擎之间的唯一方向契约。

#[derive(Debug, Clone, Copy, PartialEq)]
/// 策略信号。
///
/// 由信号序列展开为持仓方向; `Hold` 维持上一方向, `Close` 平仓。
/// `ExitLong` / `ExitShort` 只平掉对应方向的持仓: 当前目标是多头时
/// `ExitLong` 等价于 `Close`, 当前目标是空头或空仓时保持目标不变。
pub enum Signal {
    /// 维持当前方向。
    Hold,
    /// 开多(或由空翻多)。
    Buy,
    /// 开空(或由多翻空)。
    Sell,
    /// 平仓, 之后转空仓。
    Close,
    /// 仅平多: 目标为多时转空仓, 其余目标不变。
    ExitLong,
    /// 仅平空: 目标为空时转空仓, 其余目标不变。
    ExitShort,
}
