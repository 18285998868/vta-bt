use std::path::PathBuf;

use crate::config;
use crate::output;
use crate::strategies::bars_per_year;
use crate::strategies::build::build_jobs_checked;

use super::combo::*;
use super::fingerprint::*;

/// 整体逻辑编排(与代码实际结构逐行对应):
/// ```text
/// 1.  加载配置并校验: config::load_config(path)?; config.validate()?; build_strategies → strategies
/// 2.  for symbol in data.symbols()                        // 品种层(单值时长度为 1)
/// 3.    for tf in data.timeframes()                       // 周期层(单值时长度为 1)
/// 4.      cols ← data.for_pair(symbol, tf).load()                // 逐对加载
/// 5.      for strategy in strategies                      // 策略层
/// 6.        run_combo(...) → 单个组合处理, 内部:
/// 7.          a. 跳过判断: 非 overwrite 且组合缓存文件(含参数指纹)可读 → 跳过(仍并入合并)
/// 8.          b. 参数并行: 按信号内存预算分批, 批内并行 生成信号 + simulate(/detailed)
///8c.   data_fp = data_fingerprint(cols)   // 策略层循环**外**算一次, 供全部策略复用
/// 9.          c. detail 模式收集 trades/equity_curve
/// 10.         d. 写出: write_single_parquet(temps/) [+ write_detail_parquet(detail/)]
/// 11. merge_parquet_files(combo_files) → result.parquet  // 只合并本次运行组合(新算+跳过)
/// ```
///
/// 单个 (symbol, timeframe) 数据加载失败时跳过该对并继续, 不中断整体扫描;
/// 全部组合都失败才返回错误。
///
/// # Errors
/// 配置加载/校验失败、参数网格自检失败、数据列非法、策略批量信号长度违反契约,
/// 或报告合并阶段完全失败时返回 [`crate::error::AppError`]。
pub fn run(config_path: &std::path::Path) -> crate::error::Result<()> {
    run_with(config_path, RunOptions::default())
}

/// [`run`] 的运行期选项(命令行可控)。
///
/// 单独成结构体而非给 `run` 加布尔参数: 布尔参数在调用点不可读
/// (`run(&p, true)` 看不出 `true` 是什么), 且再加开关就得再改签名。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// **忽略全部缓存**, 强制重算。
    ///
    /// 正常情况下缓存已足够安全: 指纹覆盖行情内容、资金、窗口、执行口径、
    /// 产出开关、**实现版本**(源码内容哈希) ⇒ 代码/数据/配置任一变化都会
    /// 自动失效。本开关用于"明知缓存可能过期、但只想强制重算一次"的场合。
    pub no_cache: bool,
}

/// 带选项执行一轮回测。生产入口见 [`run`]。
///
/// # Errors
/// 配置加载/校验失败、数据加载失败、或回测写盘失败时返回 `AppError`。
pub fn run_with(config_path: &std::path::Path, opts: RunOptions) -> crate::error::Result<()> {
    let config = config::load_config(config_path)?;
    config.validate()?;
    // 必须走 `build_jobs_checked`: 它构建策略 + 该条配置的 stop, 并在每条策略内
    // 对展开网格做 `param_str` 唯一性自检。直接用 `build_strategies` 会丢掉 stop,
    // 也静默放过 parquet `strategy_params` 列碰撞(见 build.rs 的文档)。
    let jobs = build_jobs_checked(&config.strategy)?;
    let detail = config.output.as_ref().is_some_and(|o| o.detail);
    let fixed_curves = config.output.as_ref().is_some_and(|o| o.fixed_curves);
    let overwrite = config.output.as_ref().is_some_and(|o| o.overwrite);
    let balance = config.backtest.initial_balance.amount();
    let window = format!(
        "{}~{}",
        config.data.start_date.as_deref().unwrap_or(""),
        config.data.end_date.as_deref().unwrap_or("")
    );

    let symbols = config.data.symbols();
    let timeframes = config.data.timeframes();
    let pairs_total = symbols.len() * timeframes.len();

    let out_dir: PathBuf = config
        .output
        .as_ref()
        .and_then(|o| o.dir.clone())
        .unwrap_or_else(|| PathBuf::from("reports"));
    let temps_dir = out_dir.join("temps");
    let detail_dir = out_dir.join("detail");
    // detail 索引标记"当前报告目录的 detail 产物属于哪一轮"。运行一开始就删掉:
    // 若本轮中途失败, 旧索引会继续指向可能已被部分覆盖的曲线, `--pbo` 会读到
    // 混合数据。删掉后 PBO 会明确要求重跑, 而不是静默给出错误 φ。
    // 非 detail 运行同样删除 —— 本轮没有产出 detail, 旧 detail 与新的
    // result.parquet 不再属于同一次运行。
    let detail_index_path = detail_dir.join(output::DETAIL_INDEX_FILE);
    match std::fs::remove_file(&detail_index_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(crate::error::AppError::Msg(format!(
                "删除旧 detail 索引失败 {}: {e}",
                detail_index_path.display()
            )));
        }
    }

    let mut combo_files: Vec<PathBuf> = Vec::new();
    let mut skipped = 0u64;
    let mut ran = 0u64;
    let mut load_failed = 0usize;

    let ctx = ComboCtx {
        balance,
        detail,
        fixed_curves,
        overwrite,
        no_cache: opts.no_cache,
        temps_dir: temps_dir.clone(),
        detail_dir: detail_dir.clone(),
        window: window.clone(),
        exec: crate::engine::ExecConfig::try_from_bps_fill(
            config.backtest.delay_bars.get(),
            config.backtest.fee_bps.get(),
            config.backtest.slippage_bps.get(),
            config.backtest.fill_bar_price == config::FillBarPrice::Close,
        )?,
    };

    tracing::info!("\n{}", "=".repeat(70));
    tracing::info!(
        "  扫描规模: {} 品种 × {} 周期 × {} 策略 = {} 组",
        symbols.len(),
        timeframes.len(),
        jobs.len(),
        pairs_total * jobs.len()
    );
    tracing::info!("{}", "=".repeat(70));

    let mut pair_idx = 0usize;
    for symbol in symbols {
        for tf in timeframes {
            pair_idx += 1;
            tracing::info!("\n[{pair_idx}/{pairs_total}] {symbol} / {tf}");

            let pair_config = match config.data.for_pair(symbol, tf) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("[跳过] 配置无效: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            let cols = match pair_config.load() {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("[跳过] 数据加载失败: {e}");
                    load_failed += 1;
                    continue;
                }
            };
            if cols.is_empty() {
                tracing::warn!("[跳过] 无数据");
                load_failed += 1;
                continue;
            }
            tracing::info!("数据: {} bars", cols.len());

            let bpy = bars_per_year(tf);
            // 策略层循环外算一次: 同一份 `cols` 会被全部策略复用, 在 `run_combo`
            // 内部重算等于把一次全列哈希乘以策略数(100+ 策略 ⇒ 100 倍浪费)。
            let data_fp = data_fingerprint(&cols);
            // 列合法性检查同理**提到策略循环外**: 它逐列全量扫描 4 条价格列
            // (O(T)), 放进 `run_combo` 等于把这次扫描乘以策略数。
            // 语义不变 —— 同一份 `cols` 对全部策略的判定本就相同, 提前失败
            // 反而更早报错(且不依赖是否走到策略层)。
            validate_columns(&cols, &format!("{symbol}/{tf}"))?;
            for job in &jobs {
                let (combo_path, computed) = run_combo(job, symbol, tf, &cols, bpy, data_fp, &ctx)?;
                if computed {
                    ran += 1;
                } else {
                    skipped += 1;
                }
                combo_files.push(combo_path);
            }
        }
    }

    if combo_files.is_empty() {
        return Err(crate::error::AppError::Msg(format!(
            "全部 {} 组 (品种 × 周期) 均加载失败, 无可回测数据; 请检查 data.dir / symbol / timeframe",
            load_failed
        )));
    }

    let out_path = out_dir.join("result.parquet");
    let stats = output::merge_parquet_files(&combo_files, &out_path, balance)?;
    // `saturating_sub`: `total_files` 是 `u32::try_from(len).unwrap_or(u32::MAX)`,
    // 当组合文件数超过 `u32::MAX` 时它被截成 `u32::MAX`, 而 `failed_files`
    // 仍按真实失败数累加 ⇒ 普通 `-` 在 debug 下会**下溢 panic**。
    let ok_files = stats.total_files.saturating_sub(stats.failed_files);
    tracing::info!(
        "\n  完成: 运行 {ran} 组, 跳过 {skipped} 组, 合并 {} 条结果 (来自 {}/{} 个文件)",
        stats.merged_rows,
        ok_files,
        stats.total_files
    );
    if stats.failed_files > 0 {
        // 报告缺了参数, 必须**以 warn 级别**显式告知 —— 用 `info!` 的话,
        // 默认的 `RUST_LOG=info` 恰好能看见, 但生产常见的 `RUST_LOG=warn`
        // 会把它**整个滤掉**, 于是"报告不完整"这件事无人知晓。
        tracing::warn!(
            "  [警告] {} 个组合文件读取失败已被跳过, 报告不完整(请删 temps/ 后重跑)",
            stats.failed_files
        );
    }
    if load_failed > 0 {
        tracing::info!("跳过 {load_failed} 组数据缺失的 (品种 × 周期)");
    }
    tracing::info!("合并 Parquet 已保存: {}", out_path.display());
    if detail {
        output::write_detail_index(&detail_dir, &combo_files)?;
        tracing::info!("详细数据 Parquet 已保存至: {}", detail_dir.display());
        tracing::info!("detail 索引: {}", detail_index_path.display());
    }

    Ok(())
}
