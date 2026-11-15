//! 策略可见的行情视图(六列, 生产路径保证等长)。

/// 一次信号生成所需的行情视图(六列, 生产路径保证等长)。
///
/// 历史故障: `generate_signals_full` 曾有 6 个同类型切片参数, 编译器无法发现
/// 顺序错位 —— 移除 close-only 入口时 high/low 被插到 open 之前, 数据被喂错列
/// 却无任何报错。收成具名结构后, 列顺序只在 [`Bars::new`] 的构造点确定, 策略
/// 侧按名字取值。
///
/// 单元测试可只提供实际使用的列(其余传空切片); 生产路径由
/// `KlineColumns` 保证六列等长。
#[derive(Debug, Clone, Copy)]
pub struct Bars<'a> {
    open: &'a [f64],
    high: &'a [f64],
    low: &'a [f64],
    close: &'a [f64],
    volume: &'a [f64],
    time: &'a [i64],
}

impl<'a> Bars<'a> {
    /// 完整六列视图。
    ///
    /// 参数顺序即 [`Bars`] 的列顺序: `open, high, low, close, volume, time`。
    #[must_use]
    pub const fn new(
        open: &'a [f64],
        high: &'a [f64],
        low: &'a [f64],
        close: &'a [f64],
        volume: &'a [f64],
        time: &'a [i64],
    ) -> Self {
        Self {
            open,
            high,
            low,
            close,
            volume,
            time,
        }
    }

    /// 开盘价。
    #[must_use]
    pub const fn open(&self) -> &'a [f64] {
        self.open
    }

    /// 最高价。
    #[must_use]
    pub const fn high(&self) -> &'a [f64] {
        self.high
    }

    /// 最低价。
    #[must_use]
    pub const fn low(&self) -> &'a [f64] {
        self.low
    }

    /// 收盘价。
    #[must_use]
    pub const fn close(&self) -> &'a [f64] {
        self.close
    }

    /// 成交量。
    #[must_use]
    pub const fn volume(&self) -> &'a [f64] {
        self.volume
    }

    /// 时间戳(**毫秒**, UTC)。
    #[must_use]
    pub const fn time(&self) -> &'a [i64] {
        self.time
    }

    /// 以 `close` 为准的 K 线根数。
    #[must_use]
    pub const fn len(&self) -> usize {
        self.close.len()
    }

    /// 是否没有任何 K 线。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.close.is_empty()
    }
}

#[cfg(test)]
mod bars_tests {
    use super::Bars;

    /// 六个访问器必须与 [`Bars::new`] 的形参顺序一致。
    ///
    /// 用内容互不相同的切片做探针: 若结构体字段被接错, 访问器会读到别的列。
    #[test]
    fn bars_accessors_preserve_column_order() {
        let open = [1.0];
        let high = [2.0];
        let low = [3.0];
        let close = [4.0];
        let volume = [5.0];
        let time = [6_i64];
        let bars = Bars::new(&open, &high, &low, &close, &volume, &time);

        assert_eq!(bars.open(), &open[..]);
        assert_eq!(bars.high(), &high[..]);
        assert_eq!(bars.low(), &low[..]);
        assert_eq!(bars.close(), &close[..]);
        assert_eq!(bars.volume(), &volume[..]);
        assert_eq!(bars.time(), &time[..]);
        assert_eq!(bars.len(), 1);
        assert!(!bars.is_empty());
    }
}
