use std::path::PathBuf;

use crate::engine;
use crate::output;
use crate::strategies::build::StrategyJob;

use super::fingerprint::*;
use super::types::*;

/// 单个组合执行所需的运行期上下文: 由 run() 一次性构建, 全部组合共享。
pub(super) struct ComboCtx {
    /// 初始资金(**金额**, 定点十进制)。
    pub(super) balance: crate::money::Amount,
    pub(super) detail: bool,
    pub(super) fixed_curves: bool,
    pub(super) overwrite: bool,
    /// `--no-cache`: **忽略全部缓存**强制重算(逃生门)。
    ///
    /// 与 `overwrite` 的区别: `overwrite` 是**配置项**("结果文件要不要覆盖"),
    /// 本字段是**命令行开关**, 且作用在**组合缓存**(`temps/`)上 ——
    /// `overwrite=true` 本来就意味着全部重算, 但它写在配置文件里;
    /// `--no-cache` 让用户在命令行临时强制一次, 不必改配置。
    pub(super) no_cache: bool,
    pub(super) temps_dir: PathBuf,
    pub(super) detail_dir: PathBuf,
    /// 数据窗口标识 (start_date/end_date), 纳入缓存指纹以区分不同数据区间。
    pub(super) window: String,
    pub(super) exec: crate::engine::ExecConfig,
}

impl ComboCtx {
    /// 构造一行 [`RunResult`]。**唯一构造点**。
    ///
    /// 此前 detail 与非 detail 两条分支各写一份 16 字段的字面量。两条分支的字段
    /// 集合已经在 L5(O6/bars_used/status)改造中发生过漂移风险 —— 任何一次新增
    /// 字段都可能只改到其中一条。这里收敛成单一来源后, 新增字段必然两分支同时生效。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_row(
        &self,
        symbol: &str,
        tf: &str,
        strategy: &dyn crate::strategies::Strategy,
        params: &str,
        mut metrics: engine::SummaryMetrics,
        bars_used: usize,
        status: RunStatus,
        bh_pnl: crate::money::Amount,
        bh_ret: f64,
    ) -> RunResult {
        // 执行口径是"本次运行假设", 不是从交易序列算出的指标; 在唯一构造点
        // 统一回填, 保证 invalid / 数据不足 / 正常行三者的三列完全一致。
        metrics.apply_exec(&self.exec);
        RunResult {
            symbol: symbol.to_string(),
            timeframe: tf.to_string(),
            strategy: strategy.name().to_string(),
            params: params.to_string(),
            strategy_params: output::strategy_params_value(params),
            strategy_params_json: output::params_to_json(params),
            metrics,
            delay_bars: self.exec.delay_u32(),
            fee_bps: self.exec.fee_rate * 10_000.0,
            slippage_bps: self.exec.slip_rate * 10_000.0,
            fill_close: self.exec.fill_close,
            buy_hold_pnl: bh_pnl,
            buy_hold_return: bh_ret,
            bars_used: u32::try_from(bars_used).unwrap_or(u32::MAX),
            status,
        }
    }

    /// 构造一个**不可计算**档位的报告行。
    ///
    /// 与正常行共用 [`Self::run_row`] 这**唯一**构造点(见 `docs/invariants.md`
    /// 第 11 条)—— 区别只在 `status` 与 `metrics`:
    /// - `status = RunStatus::Invalid`: 明确告知"这一档**没算出来**",
    ///   而不是"跑了但没信号";
    /// - `metrics` 除 `final_equity` 与执行口径三列外全零, 但 `final_equity = balance`
    ///   (该档未开仓, 权益就是初始资金) —— 若留 0, `final_equity` 会与
    ///   `total_return = 0` 自相矛盾。
    ///
    /// `buy_hold_*` 与 `bars_used` 仍是**真实值**(基准与数据规模不因该档
    /// 不可算而改变)。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn invalid_row(
        &self,
        symbol: &str,
        tf: &str,
        strategy: &dyn crate::strategies::Strategy,
        params: &str,
        bars_used: usize,
        bh_pnl: crate::money::Amount,
        bh_ret: f64,
    ) -> RunResult {
        let metrics = engine::SummaryMetrics {
            final_equity: self.balance,
            ..engine::SummaryMetrics::default()
        };
        self.run_row(
            symbol,
            tf,
            strategy,
            params,
            metrics,
            bars_used,
            RunStatus::Invalid,
            bh_pnl,
            bh_ret,
        )
    }
}

/// K 线六列必须**等长**, 且价格列必须**有限且为正**。
///
/// 引擎内部多处用 `close.len().min(open.len())` 之类的截断来"保证"不越界。截断
/// 是静默的: 若某列短了, 引擎只会少回测几根 bar, 而**指标不会报错** —— 报告里
/// 的 bars_used 与用户预期不符, 研究结论却在无声地少算一段数据。
/// 更坏的情形是 `time` 比价列短: `scan_trades` 的 `ts(i)` 会退化成用 bar 序号
/// 当时间戳, detail 落盘的 `entry_time`/`exit_time` 变成假时间。
///
/// 价格列的合法性是**另一条**独立的红线: 成交价是整条成交链的除数
/// (`qty = cash / price`、`pnl_pct = (exit - entry) / entry`)。价格为 0 ⇒
/// `qty = inf` ⇒ 权益变 NaN ⇒ **全部指标被污染**; 而 NaN 比 0 更危险 ——
/// 0 会被读成"零波动低风险", NaN 才会暴露。
///
/// 因此在进入回测前一次性拒绝, 让问题在**数据加载边界**暴露。
///
/// 注意(2026-10): `KlineColumns` 私有化字段后, 不等长已**无法构造** ——
/// `new()` 对六列长度做一次校验。价格列则**没有**这层保证, 故这条校验
/// 是可达的、也是必要的。
pub(super) fn validate_columns(
    cols: &crate::data::KlineColumns,
    where_: &str,
) -> crate::error::Result<()> {
    let lens = [
        ("time", cols.time().len()),
        ("open", cols.open().len()),
        ("high", cols.high().len()),
        ("low", cols.low().len()),
        ("close", cols.close().len()),
        ("volume", cols.volume().len()),
    ];
    let close_len = cols.close().len();
    if let Some((name, len)) = lens.iter().find(|(_, l)| *l != close_len) {
        return Err(crate::error::AppError::Msg(format!(
            "{where_}: K 线列长度不一致 —— {name}={len}, close={close_len}; \
             会导致引擎静默截断少算 bar 或时间戳失真。请重新下载/清洗数据"
        )));
    }
    // 价格列: 必须是有限正数。逐列扫描, 报告首个非法下标以便定位。
    for (name, col) in [
        ("open", cols.open()),
        ("high", cols.high()),
        ("low", cols.low()),
        ("close", cols.close()),
    ] {
        if let Some(i) = col.iter().position(|p| !p.is_finite() || *p <= 0.0) {
            return Err(crate::error::AppError::Msg(format!(
                "{where_}: 价格列 {name} 第 {i} 根非法(须为有限正数, 实际 {}) —— \
                 该值会成为成交数量的除数, 0 或 NaN 会让全部指标变成 NaN。\
                 请重新下载/清洗数据",
                col[i]
            )));
        }
    }
    Ok(())
}

/// 策略批量信号的长度必须与参数区间一致。
///
/// 契约由 [`crate::strategies::Strategy::generate_signals_batch`] 声明, 但
/// 实现方是**插件式的**(任何 `Strategy` 实现都可能写错), 且违反后果很重:
/// 信号与参数错位 ⇒ 报告里 `params` 列与实际跑的参数对不上 ⇒ 结论完全不可信。
///
/// 原实现用 `assert_eq!` 直接 panic —— 而此时整轮回测可能已经跑了几小时,
/// 崩溃会让此前所有已算结果白费(虽有 temps/ 缓存, 但用户拿到的是"进程被杀"
/// 而不是"哪个策略违反了契约")。这里改成**返回 Err**, 把问题变成一条可读的错误。
pub(super) fn check_batch_len(name: &str, got: usize, want: usize) -> crate::error::Result<()> {
    if got != want {
        return Err(crate::error::AppError::Msg(format!(
            "[{name}] batch 信号数 {got} != 参数区间长度 {want}(策略实现违反契约)"
        )));
    }
    Ok(())
}

/// 单个参数档位的求值产物: 报告行 + detail 模式的落盘数据。
struct ParamRun {
    row: RunResult,
    detail: Option<DetailResult>,
}

/// 组合内逐参数求值的共享上下文。
///
/// 此前 detail / 非 detail 各有一份分块循环、并行闭包与结果行构造; 两条分支
/// 的任何一次改动都可能只落到其中一条。这里把"求值一档参数"收敛为唯一入口。
struct ParamEval<'a> {
    ctx: &'a ComboCtx,
    job: &'a StrategyJob,
    symbol: &'a str,
    tf: &'a str,
    ohlcv: engine::Ohlcv<'a>,
    time: &'a [i64],
    bpy: f64,
    bars_used: usize,
    bh_pnl: crate::money::Amount,
    bh_ret: f64,
}

impl ParamEval<'_> {
    /// 求值第 `i` 档参数。
    ///
    /// `Skipped` 记一行 `invalid` 并**继续**; detail 与非 detail 只差引擎入口
    /// (`simulate_ohlcv_detailed` / `simulate_ohlcv`), 指标同源故逐位一致。
    fn evaluate(
        &self,
        i: usize,
        outcome: crate::strategies::ParamOutcome,
    ) -> crate::error::Result<ParamRun> {
        let strategy = self.job.strategy.as_ref();
        // 一次分配: params 列、detail 行与日志共用同一字符串。
        let params = self
            .job
            .stop
            .apply_to(&strategy.param_str(i), &self.job.sizing);
        let opts = engine::SimOptions {
            initial_cash: self.ctx.balance,
            bars_per_year: self.bpy,
            exec: &self.ctx.exec,
            risk: &self.job.stop,
            sizing: &self.job.sizing,
        };
        match outcome {
            crate::strategies::ParamOutcome::Signals(signals) => {
                if self.ctx.detail {
                    let sim =
                        engine::simulate_ohlcv_detailed(self.time, self.ohlcv, &signals, &opts)?;
                    let status = RunStatus::classify(&signals, &sim.summary);
                    let row = self.ctx.run_row(
                        self.symbol,
                        self.tf,
                        strategy,
                        &params,
                        sim.summary,
                        self.bars_used,
                        status,
                        self.bh_pnl,
                        self.bh_ret,
                    );
                    return Ok(ParamRun {
                        row,
                        detail: Some(DetailResult {
                            symbol: self.symbol.to_string(),
                            timeframe: self.tf.to_string(),
                            strategy: strategy.name().to_string(),
                            params,
                            status,
                            trades: sim.trades,
                            equity_curve: sim.equity_curve,
                            fixed_curve: sim.fixed_curve,
                        }),
                    });
                }
                let metrics = engine::simulate_ohlcv(self.ohlcv, &signals, &opts)?;
                let status = RunStatus::classify(&signals, &metrics);
                Ok(ParamRun {
                    row: self.ctx.run_row(
                        self.symbol,
                        self.tf,
                        strategy,
                        &params,
                        metrics,
                        self.bars_used,
                        status,
                        self.bh_pnl,
                        self.bh_ret,
                    ),
                    detail: None,
                })
            }
            crate::strategies::ParamOutcome::Skipped(reason) => {
                tracing::warn!(
                    "  [{}] 参数档位 {params} 不可计算: {reason}(记为 invalid, 继续)",
                    strategy.name(),
                );
                Ok(ParamRun {
                    row: self.ctx.invalid_row(
                        self.symbol,
                        self.tf,
                        strategy,
                        &params,
                        self.bars_used,
                        self.bh_pnl,
                        self.bh_ret,
                    ),
                    detail: None,
                })
            }
        }
    }
}

/// 逐参数分块计划: 单档峰值估算 + 每批参数个数。
///
/// detail 的曲线与逐笔交易常驻内存曾导致整机 swap, 非 detail 的全量信号也曾
/// OOM; 两者估算单位不同, 但共用同一预算与分块公式。
struct ChunkPlan {
    /// 单个参数档位的估算峰值(字节)。
    per_param: u64,
    /// 每批参数个数。
    chunk: usize,
}

impl ChunkPlan {
    /// 峰值预算: 4 GiB。
    const BUDGET: u64 = 4 * 1024 * 1024 * 1024;

    /// detail: 约 `T × 200B`; 非 detail: `T × size_of::<Signal>()`。
    ///
    /// detail 单档就超预算时**拒绝执行**, 而不是跑一个注定 swap 的组合。
    fn new(name: &str, detail: bool, bars: usize, n: usize) -> crate::error::Result<Self> {
        let per_param = if detail {
            bars as u64 * 200
        } else {
            bars as u64 * size_of::<crate::strategies::Signal>() as u64
        };
        if detail && per_param > Self::BUDGET {
            return Err(crate::error::AppError::Msg(format!(
                "[{name}] detail 单 param 预估 {} 超预算 {}, 拒绝执行(请关 detail 或拆小时间窗)",
                gib(per_param),
                gib(Self::BUDGET)
            )));
        }
        let chunk = ((Self::BUDGET / per_param.max(1))
            .max(1)
            .min(n.max(1) as u64)) as usize;
        Ok(Self { per_param, chunk })
    }

    /// 记录分块决策。detail 每次都打印预检; 非 detail 只在确实分块时打印。
    fn log(&self, name: &str, n: usize, detail: bool) {
        let batches = n.div_ceil(self.chunk.max(1));
        if detail {
            tracing::info!(
                "  [{name}] detail 预检: 总量约 {}, 分 {batches} 批, 峰值约 {}",
                gib(self.per_param.saturating_mul(n as u64)),
                gib(self.per_param.saturating_mul(self.chunk as u64))
            );
        } else if n > self.chunk {
            tracing::info!("  [{name}] batch 分块: {n} 参数, 分 {batches} 批");
        }
    }
}

/// 人类可读的字节数(用于内存预算日志)。
fn gib(bytes: u64) -> String {
    if bytes < 1_073_741_824 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    }
}

/// 执行单个 (symbol, timeframe, strategy) 组合:
/// 1. 若 overwrite=false 且缓存文件(含参数指纹)已存在且可读, 直接返回(跳过)
/// 2. 该策略的 n 个参数并行执行 信号生成+回测; detail 模式收集 trades/equity_curve
/// 3. 计算完成即写入 temps/(及 detail/)
///
/// `bpy` 与 `data_fp` 由调用方按当前 (symbol, timeframe) 对提供, 不放在 `ctx` 里:
/// 两者在策略层循环中是循环不变量(同一条 `cols`), 在这里重算等于把一次
/// O(全部列字节) 的哈希乘以策略数量(见 `data_fingerprint` 的成本说明)。
/// 同理, 列合法性检查 [`validate_columns`] 也由调用方**每组只做一次** ——
/// 它要全量扫描 4 条价格列, 放在这里等于把一次 O(T) 扫描乘以策略数。
/// 返回 (组合文件路径, 是否本次计算; false 表示命中缓存被跳过)。
pub(super) fn run_combo(
    job: &StrategyJob,
    symbol: &str,
    tf: &str,
    cols: &crate::data::KlineColumns,
    bpy: f64,
    data_fp: u64,
    ctx: &ComboCtx,
) -> crate::error::Result<(PathBuf, bool)> {
    let strategy = job.strategy.as_ref();
    let stop = &job.stop;
    let n = strategy.param_count();

    let combo_path = combo_parquet_path(
        &ctx.temps_dir,
        symbol,
        tf,
        strategy.name(),
        params_fingerprint_with_stop(
            strategy,
            n,
            ctx.balance,
            &ctx.window,
            data_fp,
            &ctx.exec,
            OutputSet {
                detail: ctx.detail,
                fixed_curves: ctx.fixed_curves,
            },
            stop,
            &job.sizing,
        ),
    );

    // 组合缓存文件名本身就是本轮的身份(含源码/数据/执行口径/产出集指纹)。
    // detail 产物以它作为 metadata 里的 `combo` 字段, 使"同名参数文件被另一轮
    // 覆盖"也能在缓存命中时被发现。
    let combo_id = combo_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // L4: 指纹相同**不保证**文件完整 —— 文件可能被截断/写坏/被旧版本写入。
    // 少行的话下游合并会得到"缺一半参数"的结果而毫无提示, 故行数也必须对得上。
    if !ctx.overwrite && !ctx.no_cache && combo_path.exists() {
        // 只读 footer 行数, 不解码整张表: 这条路径每个策略都会走一次, 而缓存命中
        // 时真正需要的只有"行数是否等于参数数"。完整的 schema/内容校验留给
        // merge 阶段的 `read_parquet`。
        match output::parquet_row_count(&combo_path) {
            Ok(rows) if rows == n => {
                // 组合 parquet 只是结果的一部分: detail 模式还承诺 trades /
                // equitycurve(/fixedcurve) 文件。历史上只看行数就跳过, 于是
                // 手动清理 detail/ 后再跑会全部命中缓存、一个 detail 文件都不补,
                // 而 `--pbo` 只能报"未找到曲线"。侧车清单 + 文件大小/所属组合
                // 校验把"缓存命中"限定为**整个产出集**都完整。
                if !ctx.detail || output::detail_cache_is_complete(&combo_path, &ctx.detail_dir)? {
                    tracing::debug!("[{}]: 已存在, 跳过 ({rows} 条结果)", strategy.name());
                    return Ok((combo_path, false));
                }
                tracing::warn!(
                    "  [{}] 组合缓存命中但 detail 产物缺失/过期, 本次重算该组合",
                    strategy.name()
                );
            }
            Ok(rows) => {
                tracing::warn!(
                    "  [{}] 缓存行数 {rows} != 参数数 {n}(文件被截断或指纹碰撞), 判为失效并重算",
                    strategy.name()
                );
            }
            Err(e) => {
                tracing::warn!("[{}] 读取失败: {e}, 重新运行", strategy.name());
            }
        }
    }

    use rayon::prelude::*;

    if ctx.detail {
        // 旧清单必须先失效, 再覆盖任何 detail 文件。否则中途崩溃会留下
        // "旧清单 + 新 detail + 旧组合 parquet" 的假完整缓存。
        output::invalidate_combo_detail_manifest(&combo_path)?;
    }

    let (bh_pnl, bh_ret) = engine::buy_hold_pnl(cols.close(), ctx.balance)?;
    // detail 与非 detail 的峰值估算单位不同(曲线 vs 信号), 但预算与分块公式一致。
    let plan = ChunkPlan::new(strategy.name(), ctx.detail, cols.len(), n)?;
    plan.log(strategy.name(), n, ctx.detail);

    let eval = ParamEval {
        ctx,
        job,
        symbol,
        tf,
        ohlcv: engine::Ohlcv::new(cols.open(), cols.high(), cols.low(), cols.close()),
        time: cols.time(),
        bpy,
        bars_used: cols.len(),
        bh_pnl,
        bh_ret,
    };

    let mut run_rows: Vec<RunResult> = Vec::with_capacity(n);
    let mut detail_files: Vec<PathBuf> = Vec::new();
    for start in (0..n).step_by(plan.chunk.max(1)) {
        let end = (start + plan.chunk).min(n);
        let batch = strategy.generate_signals_batch(
            cols.open(),
            cols.high(),
            cols.low(),
            cols.close(),
            cols.volume(),
            cols.time(),
            start..end,
        )?;
        check_batch_len(strategy.name(), batch.len(), end - start)?;
        // 每档独立: `Skipped` = 该档不可计算(记 Invalid, **继续**跑),
        // `Signals` = 正常回测。
        let evaluated = batch
            .into_par_iter()
            .enumerate()
            .map(|(k, outcome)| eval.evaluate(start + k, outcome))
            .collect::<crate::error::Result<Vec<_>>>()?;

        if ctx.detail {
            let mut detail_rows = Vec::with_capacity(evaluated.len());
            for run in evaluated {
                run_rows.push(run.row);
                // 不可计算的档位**没有**逐笔交易与曲线 → 不产出 detail 产物,
                // 但仍写一行 `status = invalid` 的结果行(让报告完整可见)。
                if let Some(detail) = run.detail {
                    detail_rows.push(detail);
                }
            }
            detail_files.extend(output::write_detail_parquet(
                &detail_rows,
                &ctx.detail_dir,
                ctx.fixed_curves,
                &combo_id,
            )?);
        } else {
            debug_assert!(
                evaluated.iter().all(|run| run.detail.is_none()),
                "非 detail 路径不得物化逐笔交易与曲线"
            );
            run_rows.extend(evaluated.into_iter().map(|run| run.row));
        }
    }

    if ctx.detail {
        detail_files.sort();
        detail_files.dedup();
    }
    output::write_single_parquet(&run_rows, &combo_path, ctx.balance)?;
    if ctx.detail {
        // 组合 parquet 与侧车清单的写入顺序: 先写组合, 再写清单。若中途崩溃,
        // 下次缓存命中会因缺清单而重算; 反过来则会留下"清单说文件齐全, 但组合
        // parquet 不存在"的假缓存。
        output::write_combo_detail_manifest(&combo_path, &ctx.detail_dir, &detail_files)?;
    }

    Ok((combo_path, true))
}
