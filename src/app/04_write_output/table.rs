use crate::app::RunResult;

pub fn sort_run_results(results: &mut [RunResult]) {
    results.sort_unstable_by(|a, b| {
        b.metrics
            .sqn
            .partial_cmp(&a.metrics.sqn)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

pub fn print_run_results(strategy: &str, results: &[RunResult]) {
    if results.is_empty() {
        return;
    }
    println!("  [{strategy}]:");
    println!(
        "  {:<6} {:>10} {:>8} {:>8} {:>6} {:>8} {:>8} {:>12}",
        "Period", "Return%", "Sharpe", "DD%", "Win%", "Trades", "SQN", "Fixed PnL"
    );
    for r in results {
        let m = &r.metrics;
        println!(
            "  {:<6} {:>10.2} {:>8.2} {:>8.2} {:>6.1} {:>8} {:>8.2} {:>12.2}",
            r.params,
            m.total_return_pct,
            m.sharpe_ratio,
            m.max_drawdown_pct,
            m.win_rate,
            m.num_trades,
            m.sqn,
            m.total_fixed_pnl
        );
    }
    println!();
}
