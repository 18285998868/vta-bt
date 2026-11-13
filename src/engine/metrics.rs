//! 由唯一指标源计算 `SummaryMetrics`。

use super::scan::MetricsSource;
use super::types::SummaryMetrics;

/// 被动基准: 首根 close 买入、末根 close 卖出, 无成本, 固定名义。
/// 返回 (pnl, return)。数据不足 2 根或首价非正有限值时返回 (0, 0)。
/// 与策略 fixed 腿同为固定名义口径, 可直接对比。
///
/// `pnl` 是**金额**(定点十进制), `return` 是**比率**(无量纲 ⇒ `f64`)。
pub fn buy_hold_pnl(
    close: &[f64],
    nominal: crate::money::Amount,
) -> crate::error::Result<(crate::money::Amount, f64)> {
    if close.len() < 2 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let (first, last) = (close[0], close[close.len() - 1]);
    if !first.is_finite() || !last.is_finite() || first <= 0.0 {
        return Ok((crate::money::Amount::ZERO, 0.0));
    }
    let ret = last / first - 1.0;
    // 金额 = 名义 × 收益率; 名义是金额, 收益率是无量纲比率。
    let pnl = nominal.mul_ratio(ret)?;
    Ok((pnl, ret))
}

/// 由**唯一**指标源计算全部 `SummaryMetrics`。
///
/// 签名只收 [`MetricsSource`] —— 落盘产物(`trades` / 曲线)在类型上不可达, 故
/// 指标**不可能**依赖 `--detail` 开关。
pub(super) fn compute_metrics(
    out: &MetricsSource,
    initial_cash: crate::money::Amount,
    bars_per_year: f64,
) -> crate::error::Result<SummaryMetrics> {
    let n = out.eq.len();
    if n < 2 {
        return Ok(SummaryMetrics::default());
    }

    // 曲线统计全部来自在线累加器(与物化口径逐位一致, 见 CurveAccum 的文档)。
    let st = out.eq.stats(bars_per_year);
    // 末点权益取**账本精确值**, 而不是 f64 曲线的最后一点: 后者再经
    // `Amount::from_f64` 回造会丢掉 15 位之后的有效数字, 与逐笔 pnl 之和对不上。
    let final_eq = out.final_equity.to_f64();
    let total_return = if initial_cash > crate::money::Amount::ZERO {
        out.final_equity.minus(initial_cash)?.to_f64() / initial_cash.to_f64()
    } else {
        0.0
    };
    let max_dd = st.max_dd;
    let pnls = &out.acc.pnls;

    let num_bars = (n - 1) as f64;
    let years = if bars_per_year > 0.0 {
        num_bars / bars_per_year
    } else {
        0.0
    };
    let sharpe = st.sharpe;
    let sortino = st.sortino;
    let volatility = st.volatility;

    let cagr = if bars_per_year > 0.0 && n > 1 {
        let years = num_bars / bars_per_year;
        if years > 0.0 && initial_cash.to_f64() > 0.0 && final_eq > 0.0 {
            // CAGR 是**比率**(无量纲)⇒ f64; 金额相除时取各自的 f64 视图。
            (final_eq / initial_cash.to_f64()).powf(1.0 / years) - 1.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let calmar = if max_dd > 0.0 { cagr / max_dd } else { 0.0 };

    let recovery_factor = if max_dd > 0.0 {
        total_return / max_dd
    } else {
        0.0
    };

    let num_trades = out.acc.num_trades;
    // avg_mae / avg_mfe / avg_hold_bars 都进报告列, 故 TradeAccum 同时累计
    // 这三个标量 —— 不必为它们保留整个 Vec<Trade>。
    let mut num_wins = 0u32;
    let mut num_losses = 0u32;
    // 金额累计走 `Amount`(定点十进制); 下面的 `pnls` 序列是**统计量**口径(f64)。
    let total_win = out.acc.total_win;
    let total_loss = out.acc.total_loss;
    let mut cw = 0u32;
    let mut cl = 0u32;
    let mut max_cw = 0u32;
    let mut max_cl = 0u32;

    let mut pnl_m2 = 0.0;
    let mut pnl_m3 = 0.0;
    let mut pnl_m4 = 0.0;
    let mut runs = 0u32;
    let mut prev_win: Option<bool> = None;

    let nt = num_trades as f64;
    let pnl_mean = if nt > 0.0 {
        pnls.iter().sum::<f64>() / nt
    } else {
        0.0
    };

    for &p in pnls.iter() {
        // 保本(p == 0)既不算赢也不算亏, 且**不打断**连胜/连亏段; runs 检验
        // 也直接跳过它(带并列值的符号序列没有唯一的 runs 定义, 忽略是标准做法)。
        if p > 0.0 {
            num_wins += 1;
            cw += 1;
            cl = 0;
            if cw > max_cw {
                max_cw = cw;
            }
        } else if p < 0.0 {
            num_losses += 1;
            cl += 1;
            cw = 0;
            if cl > max_cl {
                max_cl = cl;
            }
        }
        if p != 0.0 {
            let cur_win = p > 0.0;
            match prev_win {
                None => runs = 1,
                Some(prev) if prev != cur_win => runs += 1,
                _ => {}
            }
            prev_win = Some(cur_win);
        }

        let d = p - pnl_mean;
        let d2 = d * d;
        pnl_m2 += d2;
        pnl_m3 += d2 * d;
        pnl_m4 += d2 * d2;
    }

    let avg_win = if num_wins > 0 {
        total_win.div_count(i64::from(num_wins))?
    } else {
        crate::money::Amount::ZERO
    };
    let avg_loss = if num_losses > 0 {
        total_loss.div_count(i64::from(num_losses))?
    } else {
        crate::money::Amount::ZERO
    };
    // 下面的比值类指标是**无量纲比率** ⇒ f64。
    let profit_factor = if total_loss > crate::money::Amount::ZERO {
        total_win.to_f64() / total_loss.to_f64()
    } else {
        total_win.to_f64().max(0.0)
    };
    let expectancy = if num_trades > 0 {
        out.acc.pnl_pct_sum / f64::from(num_trades)
    } else {
        0.0
    };

    let avg_mae = if nt > 0.0 { out.acc.sum_mae / nt } else { 0.0 };
    let avg_mfe = if nt > 0.0 { out.acc.sum_mfe / nt } else { 0.0 };
    let edge_ratio = if avg_mae > 0.0 {
        avg_mfe / avg_mae
    } else {
        0.0
    };
    let avg_hold_bars = if nt > 0.0 { out.acc.sum_hold / nt } else { 0.0 };

    let wl_ratio = if avg_loss > crate::money::Amount::ZERO {
        avg_win.to_f64() / avg_loss.to_f64()
    } else {
        0.0
    };

    let skewness = if nt > 2.0 && pnl_m2 > 0.0 {
        // 总体矩 m2 = Σd²/n、m3 = Σd³/n; g1 = m3 / m2^1.5 是总体偏度。
        // 再乘 Fisher-Pearson 小样本校正 √(n(n-1))/(n-2)。
        // 旧实现把**样本**标准差 √(m2/(n-1)) 当作 m2^0.5, 结果比标准校正偏度
        // 小 ((n-1)/n)^1.5(n=10 时约低 14%)。
        let m2 = pnl_m2 / nt;
        let m3 = pnl_m3 / nt;
        let g1 = m3 / m2.powf(1.5);
        g1 * (nt * (nt - 1.0)).sqrt() / (nt - 2.0)
    } else {
        0.0
    };

    let kurtosis = if nt > 3.0 && pnl_m2 > 0.0 {
        // 总体矩 m2/m4 + Fisher 小样本校正:
        //   ((n²-1) / ((n-2)(n-3))) · m4/m2² - 3(n-1)² / ((n-2)(n-3))
        // 旧实现在第一项多乘了一个 n, 峰度被系统性放大。
        let n = nt;
        let m2 = pnl_m2 / n;
        let m4 = pnl_m4 / n;
        ((n * n - 1.0) / ((n - 2.0) * (n - 3.0))) * (m4 / (m2 * m2))
            - 3.0 * (n - 1.0) * (n - 1.0) / ((n - 2.0) * (n - 3.0))
    } else {
        0.0
    };

    let sqn = if nt > 1.0 && pnl_m2 > 0.0 {
        let pnl_std = (pnl_m2 / (nt - 1.0)).sqrt();
        if pnl_std > 0.0 {
            // SQN = √n · mean/stdev (Van Thorp, "The Risk of Ruin" 1998)。
            // √n 是定义中**有意的**统计显著性因子, 不可封顶: 原实现用 min(n,100)
            // 会让 n=200 与 n=20000 得到相同 SQN, 抹掉"证据量"这一维度。
            // 小样本的可读性由"配对 num_trades 列 + 门槛"解决(Van Thorp 建议 n>=30)。
            pnl_mean / pnl_std * nt.sqrt()
        } else {
            0.0
        }
    } else {
        0.0
    };

    let pf_stability = {
        let half = num_trades as usize / 2;
        if half >= 10 {
            let pf1 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[..half] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let pf2 = {
                let mut w = 0.0;
                let mut l = 0.0;
                for &p in &pnls[half..] {
                    if p > 0.0 {
                        w += p;
                    } else {
                        l += p.abs();
                    }
                }
                if l > 0.0 { w / l } else { w.max(0.0) }
            };
            let (mn, mx) = if pf1 < pf2 { (pf1, pf2) } else { (pf2, pf1) };
            if mx > 0.0 { mn / mx } else { 0.0 }
        } else {
            0.0
        }
    };

    let (z_score, p_value) = if num_trades > 1 && num_wins > 0 && num_losses > 0 {
        let nw = num_wins as f64;
        let nl = num_losses as f64;
        let n = num_trades as f64;
        let e_runs = 2.0 * nw * nl / n + 1.0;
        let var_runs = 2.0 * nw * nl * (2.0 * nw * nl - n) / (n * n * (n - 1.0));
        let std_runs = var_runs.sqrt();
        let z = if std_runs > 0.0 {
            (runs as f64 - e_runs) / std_runs
        } else {
            0.0
        };
        let p = 2.0 * (1.0 - normal_cdf(z.abs()));
        (z, p)
    } else {
        (0.0, 1.0)
    };

    tracing::debug!(
        num_trades,
        total_return_pct = total_return * 100.0,
        sharpe_ratio = sharpe,
        max_drawdown_pct = max_dd * 100.0,
        sqn,
        "metrics computed"
    );

    // fixed 腿统计: 以逐笔 fixed_pnl 与 fixed 盯市曲线为准, 与复利腿同公式可比。
    // 研究报告统一引用 fixed_*; 无后缀的复利口径仅供容量参考。
    let mut fixed_win = 0.0;
    let mut fixed_loss = 0.0;
    // fixed 腿的 win/loss 拆分。非 detailed 路径没有 Trade, 但逐笔 fixed_pnl
    // 可以从 acc 复原 —— 故把 fixed_pnl 一并留在 CloseOutcome 里。
    for &f in out.acc.fixed_pnls.iter() {
        if f > 0.0 {
            fixed_win += f;
        } else if f < 0.0 {
            fixed_loss += f.abs();
        }
    }
    let fixed_pf = if fixed_loss > 0.0 {
        fixed_win / fixed_loss
    } else {
        fixed_win.max(0.0)
    };
    let fixed_expectancy_bps = if nt > 0.0 {
        out.acc.total_fixed_pnl.to_f64() / nt / initial_cash.to_f64() * 10_000.0
    } else {
        0.0
    };
    let fx_st = out.fx.stats(bars_per_year);
    Ok(SummaryMetrics {
        total_return_pct: total_return * 100.0,
        total_return,
        sharpe_ratio: sharpe,
        sortino_ratio: sortino,
        max_drawdown_pct: max_dd * 100.0,
        // 胜率分母是**已决笔数**(胜 + 负), 保本不进分母: 把零盈亏算作
        // 一次非胜交易会系统性拉低胜率, 与"排除出亏损统计"的口径自相矛盾。
        win_rate: {
            let decided = num_wins + num_losses;
            if decided > 0 {
                num_wins as f64 / f64::from(decided) * 100.0
            } else {
                0.0
            }
        },
        profit_factor,
        calmar_ratio: calmar,
        num_trades,
        stop_loss_exits: out.acc.stop_loss_exits,
        trailing_stop_exits: out.acc.trailing_stop_exits,
        strategy_stop_exits: out.acc.strategy_stop_exits,
        avg_win,
        avg_loss,
        expectancy,
        consecutive_wins: max_cw,
        consecutive_losses: max_cl,
        final_equity: out.final_equity,
        volatility,
        recovery_factor,
        avg_hold_bars,
        avg_mae,
        avg_mfe,
        edge_ratio,
        wl_ratio,
        skewness,
        kurtosis,
        sqn,
        pf_stability,
        z_score,
        p_value,
        total_fixed_pnl: out.acc.total_fixed_pnl,
        turnover_per_year: if years > 0.0 {
            f64::from(num_trades) / years
        } else {
            0.0
        },
        // 执行口径由 simulate()/落盘层回填, 裸 compute 为旧口径 0。
        delay_bars: 0,
        fee_bps: 0.0,
        slippage_bps: 0.0,
        fixed_total_return: fx_st.total_return,
        fixed_sharpe_ratio: fx_st.sharpe,
        fixed_max_drawdown_pct: fx_st.max_dd * 100.0,
        fixed_expectancy_bps,
        fixed_profit_factor: fixed_pf,
    })
}
pub(super) fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

pub(super) fn erf(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    sign * y
}
