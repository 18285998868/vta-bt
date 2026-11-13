//! 汇总报告(单行 Parquet)的构建与原子写入。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field};
use arrow::record_batch::RecordBatch;

use crate::app::RunResult;
use crate::error::Result;

/// 构造结果表的 Arrow `RecordBatch`。
///
/// # Errors
/// Arrow schema 与列数不匹配时返回 `AppError`(crate::error::AppError)。
///
/// 此前这里用 `.expect("Parquet schema/column count mismatch")` 直接 panic, 而
/// `write_parquet` 的 `# Errors` 段却承诺"schema 与列数不匹配时返回 `AppError`(crate::error::AppError)"——
/// **文档与实现直接矛盾**。且 `cols` 与 `fields` 是两份手工同步的平行数组
/// (各 53 项), 加一列忘了改另一列就会在**生产路径**上崩掉整轮回测。
pub(super) fn build_parquet(
    results: &[RunResult],
    balance: crate::money::Amount,
) -> Result<RecordBatch> {
    let mut strategy = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut timeframe = StringBuilder::new();
    let mut strategy_params = UInt64Builder::new();
    let mut strategy_params_json = StringBuilder::new();
    let mut params_full = StringBuilder::new();
    let mut bal = money_builder()?;
    let mut trade_size = StringBuilder::new();
    let mut total_return = Float64Builder::new();
    let mut final_equity = money_builder()?;
    let mut max_dd = Float64Builder::new();
    let mut trades = UInt32Builder::new();
    let mut stop_loss_exits = UInt32Builder::new();
    let mut trailing_stop_exits = UInt32Builder::new();
    let mut strategy_stop_exits = UInt32Builder::new();
    let mut sharpe = Float64Builder::new();
    let mut sortino = Float64Builder::new();
    let mut win_rate = Float64Builder::new();
    let mut pf = Float64Builder::new();
    let mut calmar = Float64Builder::new();
    let mut avg_win = money_builder()?;
    let mut avg_loss = money_builder()?;
    let mut expectancy = Float64Builder::new();
    let mut consecutive_wins = UInt32Builder::new();
    let mut consecutive_losses = UInt32Builder::new();
    let mut volatility_pct = Float64Builder::new();
    let mut recovery_factor = Float64Builder::new();
    let mut avg_hold_bars = Float64Builder::new();
    let mut avg_mae_price = Float64Builder::new();
    let mut avg_mfe_price = Float64Builder::new();
    let mut edge_ratio = Float64Builder::new();
    let mut wl_ratio = Float64Builder::new();
    let mut skewness = Float64Builder::new();
    let mut kurtosis = Float64Builder::new();
    let mut sqn = Float64Builder::new();
    let mut pf_stability = Float64Builder::new();
    let mut z_score = Float64Builder::new();
    let mut p_value = Float64Builder::new();
    let mut fixed_pnl = money_builder()?;
    let mut turnover_per_year = Float64Builder::new();
    let mut delay_bars = UInt32Builder::new();
    let mut fee_bps = Float64Builder::new();
    let mut slippage_bps = Float64Builder::new();
    let mut fixed_total_return = Float64Builder::new();
    let mut fixed_sharpe_ratio = Float64Builder::new();
    let mut fixed_max_drawdown = Float64Builder::new();
    let mut fixed_expectancy_bps = Float64Builder::new();
    let mut fixed_profit_factor = Float64Builder::new();
    let mut fill_close = UInt8Builder::new();
    let mut buy_hold_pnl = money_builder()?;
    let mut buy_hold_return = Float64Builder::new();
    let mut bars_used = UInt32Builder::new();
    let mut run_status = StringBuilder::new();

    for r in results {
        let m = &r.metrics;
        strategy.append_value(&r.strategy);
        symbol.append_value(&r.symbol);
        timeframe.append_value(&r.timeframe);
        // 原样写出 RunResult 携带的值: 合并时不再从 params 重新推导。
        strategy_params.append_value(r.strategy_params);
        strategy_params_json.append_value(&r.strategy_params_json);
        params_full.append_value(&r.params);
        bal.append_value(money_mantissa(balance)?);
        trade_size.append_value(trade_size_label(&r.params));
        total_return.append_value(m.total_return);
        final_equity.append_value(money_mantissa(m.final_equity)?);
        max_dd.append_value(m.max_drawdown_pct / 100.0);
        trades.append_value(m.num_trades);
        stop_loss_exits.append_value(m.stop_loss_exits);
        trailing_stop_exits.append_value(m.trailing_stop_exits);
        strategy_stop_exits.append_value(m.strategy_stop_exits);
        sharpe.append_value(m.sharpe_ratio);
        sortino.append_value(m.sortino_ratio);
        win_rate.append_value(m.win_rate / 100.0);
        pf.append_value(m.profit_factor);
        calmar.append_value(m.calmar_ratio);
        avg_win.append_value(money_mantissa(m.avg_win)?);
        avg_loss.append_value(money_mantissa(m.avg_loss)?);
        expectancy.append_value(m.expectancy);
        consecutive_wins.append_value(m.consecutive_wins);
        consecutive_losses.append_value(m.consecutive_losses);
        volatility_pct.append_value(m.volatility);
        recovery_factor.append_value(m.recovery_factor);
        avg_hold_bars.append_value(m.avg_hold_bars);
        avg_mae_price.append_value(m.avg_mae);
        avg_mfe_price.append_value(m.avg_mfe);
        edge_ratio.append_value(m.edge_ratio);
        wl_ratio.append_value(m.wl_ratio);
        skewness.append_value(m.skewness);
        kurtosis.append_value(m.kurtosis);
        sqn.append_value(m.sqn);
        pf_stability.append_value(m.pf_stability);
        z_score.append_value(m.z_score);
        p_value.append_value(m.p_value);
        fixed_pnl.append_value(money_mantissa(m.total_fixed_pnl)?);
        turnover_per_year.append_value(m.turnover_per_year);
        delay_bars.append_value(r.delay_bars);
        fee_bps.append_value(r.fee_bps);
        slippage_bps.append_value(r.slippage_bps);
        fixed_total_return.append_value(m.fixed_total_return);
        fixed_sharpe_ratio.append_value(m.fixed_sharpe_ratio);
        // 除 100 -> 与复利腿 max_drawdown 同为小数(Ratio), 单位错配已修。
        fixed_max_drawdown.append_value(m.fixed_max_drawdown_pct / 100.0);
        fixed_expectancy_bps.append_value(m.fixed_expectancy_bps);
        fixed_profit_factor.append_value(m.fixed_profit_factor);
        fill_close.append_value(u8::from(r.fill_close));
        buy_hold_pnl.append_value(money_mantissa(r.buy_hold_pnl)?);
        buy_hold_return.append_value(r.buy_hold_return);
        bars_used.append_value(r.bars_used);
        run_status.append_value(r.status.as_str());
    }

    let cols: Vec<ArrayRef> = vec![
        Arc::new(strategy.finish()),
        Arc::new(symbol.finish()),
        Arc::new(timeframe.finish()),
        Arc::new(strategy_params.finish()),
        Arc::new(strategy_params_json.finish()),
        Arc::new(bal.finish()),
        Arc::new(trade_size.finish()),
        Arc::new(total_return.finish()),
        Arc::new(final_equity.finish()),
        Arc::new(max_dd.finish()),
        Arc::new(trades.finish()),
        Arc::new(stop_loss_exits.finish()),
        Arc::new(trailing_stop_exits.finish()),
        Arc::new(strategy_stop_exits.finish()),
        Arc::new(sharpe.finish()),
        Arc::new(sortino.finish()),
        Arc::new(win_rate.finish()),
        Arc::new(pf.finish()),
        Arc::new(calmar.finish()),
        Arc::new(avg_win.finish()),
        Arc::new(avg_loss.finish()),
        Arc::new(expectancy.finish()),
        Arc::new(consecutive_wins.finish()),
        Arc::new(consecutive_losses.finish()),
        Arc::new(volatility_pct.finish()),
        Arc::new(recovery_factor.finish()),
        Arc::new(avg_hold_bars.finish()),
        Arc::new(avg_mae_price.finish()),
        Arc::new(avg_mfe_price.finish()),
        Arc::new(edge_ratio.finish()),
        Arc::new(wl_ratio.finish()),
        Arc::new(skewness.finish()),
        Arc::new(kurtosis.finish()),
        Arc::new(sqn.finish()),
        Arc::new(pf_stability.finish()),
        Arc::new(z_score.finish()),
        Arc::new(p_value.finish()),
        Arc::new(fixed_pnl.finish()),
        Arc::new(params_full.finish()),
        Arc::new(turnover_per_year.finish()),
        Arc::new(delay_bars.finish()),
        Arc::new(fee_bps.finish()),
        Arc::new(slippage_bps.finish()),
        Arc::new(fixed_total_return.finish()),
        Arc::new(fixed_sharpe_ratio.finish()),
        Arc::new(fixed_max_drawdown.finish()),
        Arc::new(fixed_expectancy_bps.finish()),
        Arc::new(fixed_profit_factor.finish()),
        Arc::new(fill_close.finish()),
        Arc::new(buy_hold_pnl.finish()),
        Arc::new(buy_hold_return.finish()),
        Arc::new(bars_used.finish()),
        Arc::new(run_status.finish()),
    ];

    let fields = vec![
        Field::new("strategy", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("timeframe", DataType::Utf8, false),
        Field::new("strategy_params", DataType::UInt64, false),
        Field::new("strategy_params_json", DataType::Utf8, false),
        Field::new(
            "balance",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("trade_size", DataType::Utf8, false),
        Field::new("total_return", DataType::Float64, false),
        Field::new(
            "final_equity",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("max_drawdown", DataType::Float64, false),
        Field::new("trades", DataType::UInt32, false),
        Field::new("stop_loss_exits", DataType::UInt32, false),
        Field::new("trailing_stop_exits", DataType::UInt32, false),
        Field::new("strategy_stop_exits", DataType::UInt32, false),
        Field::new("sharpe", DataType::Float64, false),
        Field::new("sortino", DataType::Float64, false),
        Field::new("win_rate", DataType::Float64, false),
        Field::new("profit_factor", DataType::Float64, false),
        Field::new("calmar", DataType::Float64, false),
        Field::new(
            "avg_win",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new(
            "avg_loss",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("expectancy", DataType::Float64, false),
        Field::new("consecutive_wins", DataType::UInt32, false),
        Field::new("consecutive_losses", DataType::UInt32, false),
        Field::new("volatility_pct", DataType::Float64, false),
        Field::new("recovery_factor", DataType::Float64, false),
        Field::new("avg_hold_bars", DataType::Float64, false),
        Field::new("avg_mae_price", DataType::Float64, false),
        Field::new("avg_mfe_price", DataType::Float64, false),
        Field::new("edge_ratio", DataType::Float64, false),
        Field::new("wl_ratio", DataType::Float64, false),
        Field::new("skewness", DataType::Float64, false),
        Field::new("kurtosis", DataType::Float64, false),
        Field::new("sqn", DataType::Float64, false),
        Field::new("pf_stability", DataType::Float64, false),
        Field::new("z_score", DataType::Float64, false),
        Field::new("p_value", DataType::Float64, false),
        Field::new(
            "fixed_pnl",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("params", DataType::Utf8, false),
        Field::new("turnover_per_year", DataType::Float64, false),
        Field::new("delay_bars", DataType::UInt32, false),
        Field::new("fee_bps", DataType::Float64, false),
        Field::new("slippage_bps", DataType::Float64, false),
        Field::new("fixed_total_return", DataType::Float64, false),
        Field::new("fixed_sharpe_ratio", DataType::Float64, false),
        Field::new("fixed_max_drawdown", DataType::Float64, false),
        Field::new("fixed_expectancy_bps", DataType::Float64, false),
        Field::new("fixed_profit_factor", DataType::Float64, false),
        Field::new("fill_close", DataType::UInt8, false),
        Field::new(
            "buy_hold_pnl",
            DataType::Decimal128(MONEY_PRECISION, MONEY_SCALE_I8),
            false,
        ),
        Field::new("buy_hold_return", DataType::Float64, false),
        // 退化结果标记: 见 RunStatus。bars_used 与 status 必须配对解读。
        Field::new("bars_used", DataType::UInt32, false),
        Field::new("status", DataType::Utf8, false),
    ];

    let schema = arrow::datatypes::Schema::new(fields);
    Ok(RecordBatch::try_new(Arc::new(schema), cols)?)
}

/// 金额列的 `Decimal128` builder(精度/标度固定)。
///
/// # Errors
/// 精度/标度非法时返回 `AppError`(crate::error::AppError) ——
/// 本项目的常量 (38, 8) 恒合法, 该分支只是不把 `expect` 写进生产代码。
pub(super) fn money_builder() -> Result<Decimal128Builder> {
    Ok(Decimal128Builder::new().with_precision_and_scale(MONEY_PRECISION, MONEY_SCALE_I8)?)
}

/// 金额 → Parquet `Decimal128` 尾数。
///
/// # Errors
/// 金额的小数位超过 [`MONEY_SCALE`], 落盘会丢精度 ⇒ 报错(不静默舍入)。
pub(super) fn money_mantissa(a: crate::money::Amount) -> Result<i128> {
    a.to_decimal128_mantissa(MONEY_SCALE)
}

/// Parquet `Decimal128` 的**精度**(总有效位数)。
pub(super) const MONEY_PRECISION: u8 = 38;

/// Parquet `Decimal128` 金额列的**标度**(小数位数)。
pub(super) const MONEY_SCALE: u32 = crate::money::MONEY_SCALE;

/// 同上, `i8` 形式(Arrow 的 `DataType::Decimal128` 用 `i8` 表示标度)。
pub(super) const MONEY_SCALE_I8: i8 = MONEY_SCALE as i8;

/// 报告 `trade_size` 列: 从 `params` 的仓位后缀提取人类可读的模式。
///
/// 该列是展示用摘要, 权威参数在 `params` / `strategy_params_json`。从 `params`
/// 反推而不是新增结构字段, 使旧缓存文件在 merge 时也能被纠正为真实仓位。
pub(super) fn trade_size_label(params: &str) -> String {
    let Some((_, suffix)) = params.split_once(';') else {
        return "100%".to_string();
    };
    let mut mode = None;
    let mut fraction = None;
    for kv in suffix.split(',') {
        if let Some(v) = kv.strip_prefix("size=") {
            mode = Some(v);
        } else if let Some(v) = kv.strip_prefix("size_fraction_pct=") {
            fraction = Some(v);
        }
    }
    match mode {
        Some("fixed") => format!("{}%", fraction.unwrap_or("?")),
        Some("kelly") => "kelly".to_string(),
        Some("atr") => "atr".to_string(),
        _ => "100%".to_string(),
    }
}

/// 把结果行写成单个 Parquet 文件(父目录自动创建)。
///
/// # Errors
/// IO 失败, 或 Arrow schema 与列数不匹配时返回 `AppError`(crate::error::AppError)。
pub fn write_parquet(
    path: &Path,
    results: &[RunResult],
    balance: crate::money::Amount,
) -> Result<()> {
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;

    let batch = build_parquet(results, balance)?;
    let schema = batch.schema();

    write_atomic(path, |tmp| {
        let file = File::create(tmp)?;
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    })
}

/// 原子写文件: 先写 `{path}.tmp`, 成功后再 `rename` 到 `path`。
///
/// **为什么必须原子**: `File::create` 直接**截断**目标路径。若在 `write`/
/// `close` 之间进程被杀、磁盘写满或 IO 出错, 目标文件是**半截**的, 而旧的
/// 可用版本已被销毁 —— 此后 `read_parquet` 读到的可能是"行数正好对、内容却
/// 写坏"的中间态, 而缓存有效性检查只看**行数**, 挡不住这种损坏。
///
/// 同目录 `rename` 在 POSIX 上是原子的 ⇒ 目标路径要么是旧的完整文件, 要么是
/// 新的完整文件, **不会**是中间态。
///
/// 临时文件名带 `.tmp` 后缀 ⇒ 扩展名不是 `parquet`, 不会被 `read_dir` 的
/// 过滤条件(`extension() == "parquet"`)选中, 故中途崩溃留下的临时文件不会
/// 被当成数据文件读进来。
///
/// 失败时删除临时文件, 不留垃圾。
pub(super) fn write_atomic(path: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    match write(&tmp) {
        Ok(()) => {
            std::fs::rename(&tmp, path)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// 把参数串清洗成**文件名安全且无碰撞**的片段。
///
/// 原实现是 `params.replace(' ', "_").replace('=', "-").replace(',', "_")` ——
/// 这**不是**一一映射: `"a=b"` 与 `"a-b"` 都变成 `a-b`; `"a,b"` 与 `"a_b"` 也一样。
/// 于是**不同参数组会写同一个文件、互相覆盖** —— detail 目录里少了一部分曲线,
/// 而 `--pbo` 会基于**少读一半**的曲线算出 φ, 无任何提示。
///
/// 修法: 清洗后的可读片段**追加参数串的 FNV-1a 哈希**(`{h:08x}`，8–16 位十六进制)。
/// 既保留人类可读性, 又保证不同参数串**必定**得到不同文件名。
pub(super) fn sanitize_params_for_filename(params: &str) -> String {
    let readable: String = params
        .chars()
        .map(|c| match c {
            ' ' => '_',
            '=' => '-',
            ',' => '_',
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+') => c,
            _ => '_',
        })
        .collect();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in params.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{readable}_{h:08x}")
}
