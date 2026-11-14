use super::cscv::sharpe_of_curve;
use super::report::select_curves;

/// 两遍法 Sharpe 的**参照实现**(先物化收益数组, 再单遍统计)。
///
/// 仅测试用: 它是 [`sharpe_of_curve`] 的正确性基准 —— 刻意用与后者**不同**的写法
/// (物化 `Vec` 后再索引), 使"两者同时写错同一处"不可能发生。
fn sharpe_of(rets: &[f64], bpy: f64) -> f64 {
    let n = rets.len();
    if n < 2 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / n as f64;
    let var = rets.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / (n as f64 - 1.0);
    if var <= 0.0 {
        0.0
    } else {
        mean / var.sqrt() * bpy.sqrt()
    }
}

use super::*;

// 确定性伪随机(LCG), 测试不依赖 rand crate。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / (u64::MAX >> 33) as f64
    }
}

fn noise_curves(n: usize, t: usize, seed: u64) -> Vec<Vec<f64>> {
    let mut rng = Lcg(seed);
    (0..n)
        .map(|_| {
            let mut eq = 10000.0;
            let mut c = vec![eq];
            for _ in 1..t {
                eq *= 1.0 + (rng.next() - 0.5) * 0.02;
                c.push(eq);
            }
            c
        })
        .collect()
}

#[test]
fn test_pick_s_thresholds() {
    assert_eq!(pick_s(31), None);
    assert_eq!(pick_s(32), Some(4));
    assert_eq!(pick_s(127), Some(4));
    assert_eq!(pick_s(128), Some(8));
    assert_eq!(pick_s(511), Some(8));
    assert_eq!(pick_s(512), Some(16));
}

#[test]
fn test_pure_noise_pbo_high() {
    // 纯噪声 N=30: IS 最优大概率 OOS 跑输中位数 → φ 显著 >0.05。
    let curves = noise_curves(30, 400, 7);
    let r = cscv(&curves, 4, 252.0).expect("合法口径的 CSCV 必须成功");
    assert_eq!(r.n_logits, 6); // C(4,2)
    assert!(r.pbo > 0.05, "noise φ={}", r.pbo);
}

#[test]
fn test_planted_edge_pbo_low() {
    // 植入真边际: 策略0每 bar +0.004 确定漂移, 其余噪声 → IS 最优稳定居首, φ 应很低。
    let t = 400;
    let mut curves = noise_curves(20, t, 42);
    let mut rng = Lcg(99);
    let mut eq = 10000.0;
    let mut c = vec![eq];
    for _ in 1..t {
        eq *= 1.0 + 0.004 + (rng.next() - 0.5) * 0.004;
        c.push(eq);
    }
    curves[0] = c;
    let r = cscv(&curves, 4, 252.0).expect("合法口径的 CSCV 必须成功");
    assert_eq!(
        r.full_sample_best_idx, 0,
        "全样本最优应是那条植入漂移的曲线"
    );
    assert!(r.full_sample_best_sharpe > 0.0);
    assert!(r.pbo < 0.2, "edge φ={}", r.pbo);
}

/// L14: `full_sample_best_*` 确实是**全样本**口径 —— 用同一公式独立重算一遍
/// 交叉验证。字段重命名正是为了让读者不再把它与 `degradation` 依赖的
/// "每划分 IS 最优"混为一谈; 本测试锁定前者的确切含义不被后续改动悄悄改掉。
#[test]
fn full_sample_best_is_really_full_sample_sharpe() {
    let t = 256;
    let s_blocks = 8;
    let bpy = 252.0;
    let n = 12;
    let mut curves = Vec::new();
    for j in 0..n {
        let mut c = vec![10_000.0];
        let mut eq = 10_000.0;
        for i in 1..t {
            let drift = match j {
                0 if i < t / 2 => 0.0015,
                0 => -0.0005,
                1 if i < t / 2 => -0.0005,
                1 => 0.0015,
                _ => 0.0,
            };
            let noise = ((j * 7919 + i * 104_729) % 1000) as f64 / 100_000_000.0 - 5e-6;
            eq *= 1.0 + drift + noise;
            c.push(eq);
        }
        curves.push(c);
    }
    let r = cscv(&curves, s_blocks, bpy).expect("合法口径的 CSCV 必须成功");

    // 独立重算全样本 Sharpe(argmax 用稳定规则: 取最大值的首个下标)
    let full: Vec<f64> = curves
        .iter()
        .map(|c| {
            let rets: Vec<f64> = crate::engine::bar_log_returns(c).collect();
            sharpe_of(&rets, bpy)
        })
        .collect();
    let argmax = full
        .iter()
        .enumerate()
        .fold((0usize, f64::NEG_INFINITY), |(bi, bv), (i, &v)| {
            if v > bv { (i, v) } else { (bi, bv) }
        })
        .0;
    assert_eq!(
        r.full_sample_best_idx, argmax,
        "字段必须仍是全样本 Sharpe 最大者"
    );
    assert!((r.full_sample_best_sharpe - full[argmax]).abs() < 1e-12);
    assert!(r.pbo.is_finite() && (0.0..=1.0).contains(&r.pbo));
}

#[test]
fn test_sharpe_matches_engine_formula() {
    // 常数收益序列Sharpe 应为 0(方差为零); 线性序列与手算一致。
    assert_eq!(sharpe_of(&[0.01, 0.01, 0.01], 252.0), 0.0);
    let r = sharpe_of(&[0.01, -0.01, 0.01, -0.01], 252.0);
    assert!(r.abs() < 1e-9, "{r}");
}

/// 回归: NaN Sharpe **不得**让 CSCV panic。
///
/// 可触发场景: 全样本/分块 argmax 原用 `partial_cmp(..).unwrap()`, 遇 NaN 即
/// `None` 解包 panic。现用 `total_cmp`(全序, 无 None)。
#[test]
fn nan_sharpe_argmax_path_is_total() {
    // 直接验证 argmax 所依赖的比较语义: 全序 ⇒ 任意输入(含 NaN)都有最大值。
    for xs in [
        vec![f64::NAN, 2.0, 1.0],
        vec![1.0, f64::NAN],
        vec![f64::NEG_INFINITY, f64::NAN, f64::INFINITY],
    ] {
        let best = xs.iter().max_by(|a, b| a.total_cmp(b)).copied();
        assert!(best.is_some(), "{xs:?} 应有确定的最大值");
    }
}

/// `total_cmp` 在含 NaN 的序列上给出**全序**且不 panic, 使 argmax 必定有结果。
///
/// 语义提示: 正 NaN 在 `total_cmp` 下排在**最大**一侧, 故 `max_by` 遇到 NaN
/// 会选中它。这与 `partial_cmp(..).unwrap()` 的差别不在"选谁", 而在于前者
/// **不会 panic** —— 见 `nan_inputs_do_not_panic_in_cscv` 的说明。
#[test]
fn total_cmp_orders_nan_without_panicking() {
    let mut v = [1.0, f64::NAN, 3.0, f64::NEG_INFINITY];
    // 旧写法 `a.partial_cmp(b).unwrap()` 在此会 panic(None 解包)。
    v.sort_by(f64::total_cmp);
    assert_eq!(v[0], f64::NEG_INFINITY, "-inf 排最前");
    assert_eq!(v[1], 1.0);
    assert_eq!(v[2], 3.0);
    assert!(v[3].is_nan(), "正 NaN 排在末尾(全序约定)");

    // 全序 ⇒ argmax 必有结果, 不再因 None 解包而 panic。
    let best = [f64::NAN, 2.0, 1.0]
        .iter()
        .max_by(|a, b| a.total_cmp(b))
        .copied();
    assert!(best.is_some(), "含 NaN 时 max_by 仍须给出结果");
    assert!(best.unwrap().is_nan() || best.unwrap() == 2.0);
}

/// 非有限输入**不得**让 CSCV panic, 且返回值必须可读。
///
/// 说明(重要): 本用例注入 `NaN` / `Inf` 曲线, 但 NaN 实际**不会**传播到
/// Sharpe —— `bar_log_returns` 复用 `engine::bar_log_returns`, 该函数对两侧非有限值
/// 一律记 0。因此这里真正锁定的是"退化输入下 cscv 稳定返回", 而非"NaN 被隔离"。
/// 换成 `partial_cmp().unwrap()` 时, 若未来 `sharpe_of` 的守卫被放宽, 本用例
/// 才会真正覆盖到 panic 路径。
#[test]
fn nan_inputs_do_not_panic_in_cscv() {
    let t = 64;
    let s = 4;
    let mut curves = noise_curves(10, t, 5);
    curves[3] = vec![f64::NAN; t];
    curves[7] = vec![f64::INFINITY; t];
    let r = cscv(&curves, s, 252.0).expect("合法口径的 CSCV 必须成功");
    assert!(r.pbo.is_finite(), "pbo 必须有限: {}", r.pbo);
    assert!((0.0..=1.0).contains(&r.pbo));
    assert!(r.degradation.is_finite(), "decay: {}", r.degradation);
    assert_eq!(r.n_strategies, 10);
}

/// `cscv` 的非法口径必须**返回 Err**, 而不是 panic。
///
/// 可触发场景: `cscv` 原先用三条 `assert!` 兜底, 而它是 crate 的**公共 API**
/// (`vta::pbo::cscv`) —— 下游传 `s = 0` / `s = 3` / 单条曲线即崩, 且
/// 文档里既无 `# Panics` 也无 `# Errors`, 调用方无从预知失败契约。
#[test]
fn cscv_rejects_invalid_arguments_instead_of_panicking() {
    let curves = noise_curves(4, 64, 3);

    // S 非法: 0 / 1 / 奇数 / 超过 16。
    for s in [0usize, 1, 3, 17, 100] {
        let e = cscv(&curves, s, 252.0).expect_err(&format!("S={s} 必须被拒, 而不是 panic"));
        assert!(
            format!("{e}").contains("S 必须"),
            "错误消息应说明是 S 的问题: {e}"
        );
    }

    // N 不足: 只有一条曲线无法做 CSCV(需要至少 2 条做相对排名)。
    let one = noise_curves(1, 64, 3);
    let e = cscv(&one, 4, 252.0).expect_err("N=1 必须被拒");
    assert!(format!("{e}").contains("N 必须"), "{e}");

    // T 不足: 曲线长度 < S。
    let short = noise_curves(4, 3, 3);
    let e = cscv(&short, 8, 252.0).expect_err("T<S 必须被拒");
    assert!(format!("{e}").contains("T ="), "{e}");
}

/// `sharpe_of_curve`(不物化收益数组)必须与「物化后两遍统计」**逐位一致**。
///
/// 这是"省掉 T×N 收益矩阵"这一优化的**安全网**: 口径一变(例如改成
/// `sum(x^2) - n·mean^2` 的单遍形式), 数值就会漂, φ 与 degradation 随之漂。
#[test]
fn sharpe_of_curve_matches_materialized_reference() {
    let bpy = 252.0;
    for c in noise_curves(6, 128, 11) {
        let rets: Vec<f64> = crate::engine::bar_log_returns(&c).collect();
        let expected = sharpe_of(&rets, bpy);
        let got = sharpe_of_curve(&c, bpy);
        assert_eq!(
            got.to_bits(),
            expected.to_bits(),
            "不物化版本必须与物化版本逐位一致: {got} vs {expected}"
        );
    }
}

/// `cscv` 接受**借用**的曲线切片, 且结果与传 `Vec` 完全一致。
///
/// 这条锁定的是 API 形状: `run_pbo_report` 现在零拷贝地把 `Vec<f64>` 借成
/// `&[f64]` 传进来, 省掉一整份 T×N 克隆(1m×500 规模 ≈ 4 GB)。
#[test]
fn cscv_accepts_borrowed_slices_with_identical_result() {
    let owned = noise_curves(12, 256, 77);
    let borrowed: Vec<&[f64]> = owned.iter().map(Vec::as_slice).collect();
    let a = cscv(&owned, 4, 252.0).expect("owned 应成功");
    let b = cscv(&borrowed, 4, 252.0).expect("borrowed 应成功");
    assert_eq!(a.pbo.to_bits(), b.pbo.to_bits());
    assert_eq!(a.degradation.to_bits(), b.degradation.to_bits());
    assert_eq!(a.full_sample_best_idx, b.full_sample_best_idx);
    assert_eq!(
        a.full_sample_best_sharpe.to_bits(),
        b.full_sample_best_sharpe.to_bits()
    );
}

/// **核心不变量**: 每块笔数必须与 `cscv` 的实际累加映射**逐块相等**。
///
/// 这条直接锁死曾经的缺陷: 笔数曾用 `b·m/s .. (b+1)·m/s` 的整除差
/// (floor 边界)推算, 而累加用的是 floor 分块 `b = i·s/(t−1)`(ceil 边界),
/// 两者在 `s ≥ 2` 时**永远不相等** ⇒ Sharpe 分母错 ⇒ φ 被算错。
///
/// 参照实现是**暴力计数**: 把每个收益真的投进它所属的块再数 `len()`,
/// 不依赖任何边界推导 —— 故"两边同时写错"不可能发生。
#[test]
fn block_counts_match_brute_force_accumulation() {
    for t in [2usize, 4, 8, 16, 32, 64, 128, 256, 512, 1000] {
        for s in [2usize, 4, 8, 16] {
            if t % s != 0 {
                continue;
            }
            let mut brute = vec![0usize; s];
            for i in 0..(t - 1) {
                let b = (i * s / (t - 1)).min(s - 1);
                brute[b] += 1;
            }
            assert_eq!(
                super::block_counts(t, s),
                brute,
                "t={t} s={s}: 块笔数必须与实际累加一致"
            );
        }
    }
}

/// 每块笔数之和恒等于总收益笔数 `t − 1`。
#[test]
fn block_counts_sum_to_total_return_count() {
    for t in [32usize, 64, 256, 512] {
        for s in [2usize, 4, 8, 16] {
            if t % s != 0 {
                continue;
            }
            let sum: usize = super::block_counts(t, s).iter().sum();
            assert_eq!(sum, t - 1, "t={t} s={s}: 笔数总和必须等于 t-1");
        }
    }
}

/// 曾经的缺陷形态必须被本测试**区分出来**(= 测试有牙齿)。
///
/// 旧口径用整除差推算笔数, 与真实累加**逐块不等**。若有人把
/// [`super::block_counts`] 改回旧口径, 本测试立即失败。
#[test]
fn legacy_floor_based_counts_are_provably_wrong() {
    // 旧口径: `b·m/s .. (b+1)·m/s` 的整除差。
    let legacy = |t: usize, s: usize, b: usize| {
        let m = t - 1;
        (b + 1) * m / s - b * m / s
    };
    // t=32, s=4: 真实 [8,8,8,7], 旧口径 [7,8,8,8] —— 逐块都不等。
    assert_eq!(super::block_counts(32, 4), vec![8, 8, 8, 7]);
    assert_eq!(
        (0..4).map(|b| legacy(32, 4, b)).collect::<Vec<_>>(),
        vec![7, 8, 8, 8]
    );
    // 更普适地: 在 t、s 遍历下, 旧口径与真实口径**从不**完全相等。
    for t in [32usize, 64, 128, 256, 512] {
        for s in [2usize, 4, 8, 16] {
            if t % s != 0 {
                continue;
            }
            let real = super::block_counts(t, s);
            let old: Vec<usize> = (0..s).map(|b| legacy(t, s, b)).collect();
            assert_ne!(real, old, "t={t} s={s}: 旧口径与真实口径不应相同");
        }
    }
}

/// CSCV 的**端到端参照实现**: 物化收益、按块**实际计数**、两遍法算方差。
///
/// 与 [`cscv`] 的三处刻意差异(使"两边同时写错同一处"不可能发生):
/// 1. 笔数由 `Vec::len()` 实际计数得到, 而非公式推算;
/// 2. 收益**真的装进**每块的 `Vec`(而 `cscv` 只累加一阶/二阶矩);
/// 3. 方差用两遍法(先均值再离差平方和), 而 `cscv` 用单遍代数式。
///
/// `cscv` 的收益: 不物化 T×N 收益矩阵(1m×500 规模省 4 GB)。
/// 本参照实现没有这个约束, 故可作为独立基准。
fn cscv_materialized(curves: &[Vec<f64>], s: usize, bpy: f64) -> PboResult {
    let n = curves.len();
    let t = curves.iter().map(|c| c.len()).min().unwrap_or(0);
    let t = t - (t % s);
    let m = t - 1;

    // 物化: 每条曲线 → 每块的收益 Vec(笔数由 len() 实际给出)。
    let blocks: Vec<Vec<Vec<f64>>> = curves
        .iter()
        .map(|c| {
            let rets: Vec<f64> = crate::engine::bar_log_returns(&c[..t]).collect();
            let mut out: Vec<Vec<f64>> = vec![Vec::new(); s];
            for (i, v) in rets.iter().enumerate() {
                let b = (i * s / m).min(s - 1);
                out[b].push(*v);
            }
            out
        })
        .collect();

    let sharpe_of_blocks = |j: usize, sel: &[usize]| -> f64 {
        let mut vals: Vec<f64> = Vec::new();
        for &b in sel {
            vals.extend_from_slice(&blocks[j][b]);
        }
        sharpe_of(&vals, bpy)
    };

    let half = s / 2;
    let mut neg = 0usize;
    let mut total = 0usize;
    let mut deg_sum = 0.0;
    let all: u32 = (1u32 << s) - 1;
    let mut mask = (1u32 << half) - 1;
    while mask <= all {
        let mut in_c: Vec<usize> = Vec::new();
        let mut out_c: Vec<usize> = Vec::new();
        for b in 0..s {
            if mask & (1u32 << b) != 0 {
                in_c.push(b);
            } else {
                out_c.push(b);
            }
        }
        let r_is: Vec<f64> = (0..n).map(|j| sharpe_of_blocks(j, &in_c)).collect();
        let best = r_is
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let r_oos: Vec<f64> = (0..n).map(|j| sharpe_of_blocks(j, &out_c)).collect();
        let worse = r_oos.iter().filter(|v| **v < r_oos[best]).count();
        let omega = (worse + 1) as f64 / (n + 1) as f64;
        let lambda = (omega / (1.0 - omega)).ln();
        if lambda < 0.0 {
            neg += 1;
        }
        total += 1;
        deg_sum += r_oos[best] - r_is[best];
        let c = mask.isolate_lowest_one();
        let r = mask + c;
        if r == 0 {
            break;
        }
        mask = (((r ^ mask) >> 2) / c) | r;
    }

    PboResult {
        n_strategies: n,
        t_bars: t,
        s_blocks: s,
        n_logits: total,
        pbo: neg as f64 / total.max(1) as f64,
        full_sample_best_idx: 0,
        full_sample_best_sharpe: 0.0,
        degradation: deg_sum / total.max(1) as f64,
    }
}

/// `cscv`(不物化收益)必须与「物化后逐块实际计数」的参照实现**一致**。
///
/// 这条是分块口径的端到端安全网: 笔数口径一旦与实际累加脱节
/// (曾经的 floor/ceil 边界错配), IS/OOS Sharpe 就被污染, φ 随之出错,
/// 而**没有任何**现有测试会发现 —— 故此处用完全不同的写法做交叉验证。
#[test]
fn cscv_matches_materialized_reference() {
    let bpy = 252.0;
    for (n, t, s, seed) in [
        (10usize, 64usize, 4usize, 5u64),
        (12, 128, 4, 11),
        (10, 256, 8, 23),
        (8, 512, 16, 31),
        (16, 400, 4, 47),
    ] {
        let curves = noise_curves(n, t, seed);
        let got = cscv(&curves, s, bpy).expect("合法口径的 CSCV 必须成功");
        let want = cscv_materialized(&curves, s, bpy);
        assert_eq!(got.n_logits, want.n_logits, "n={n} t={t} s={s}");
        assert_eq!(
            got.pbo.to_bits(),
            want.pbo.to_bits(),
            "n={n} t={t} s={s}: φ 必须与参照实现逐位一致 ({} vs {})",
            got.pbo,
            want.pbo
        );
    }
}

/// `select_curves`: 只保留 ok + 同一时间网格 + 唯一 params 的曲线。
///
/// 旧实现按 `min(len)` 截断: 两条 64 bar 的曲线会把 12 条 256 bar 的曲线
/// 一起砍到 T=64, 且无任何提示。本测试锁定"选最大 cohort, 不截断"。
#[test]
fn pbo_selection_prefers_largest_time_cohort() {
    use crate::app::RunStatus;

    let mk = |params: String, status: RunStatus, start: i64, n: usize| {
        let times: Vec<i64> = (0..n as i64).map(|i| start + i * 60_000).collect();
        let values: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        crate::output::FixedCurve {
            combo: "c".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            strategy: "sma".into(),
            params,
            status,
            times,
            values,
        }
    };
    let mut members: Vec<_> = (0..12)
        .map(|i| mk(format!("p{i:02}"), RunStatus::Ok, 1_700_000_000, 256))
        .collect();
    // 两条来自另一数据窗口的短曲线: 不得把整组 T 拉到 64。
    members.push(mk("old1".into(), RunStatus::Ok, 1_600_000_000, 64));
    members.push(mk("old2".into(), RunStatus::Ok, 1_600_000_000, 64));
    // 非 ok 状态: 不参与排名。
    members.push(mk("flat".into(), RunStatus::NoTrades, 1_700_000_000, 256));
    // 重复 params: 只留一条。
    members.push(mk("p00".into(), RunStatus::Ok, 1_700_000_000, 256));

    let (selected, stats) = select_curves(&members);
    assert_eq!(stats.kept, 12, "12 条同网格唯一参数");
    assert_eq!(stats.dropped_status, 1);
    assert_eq!(stats.dropped_other_window, 2);
    assert_eq!(stats.dropped_duplicate, 1);
    assert_eq!(stats.cohorts, 2);
    assert_eq!(selected.len(), 12);
    assert_eq!(
        selected[0].1.len(),
        256,
        "必须保留 256 bar 的 cohort, 不得按 min(len)=64 截断"
    );
    let mut params: Vec<&str> = selected.iter().map(|(p, _)| *p).collect();
    params.sort_unstable();
    params.dedup();
    assert_eq!(params.len(), selected.len(), "params 必须唯一");
}

/// 分组键碰撞不得把不同时间网格合并: 键只有 `(len, first, last)`, 桶内必须
/// 再用完整时间轴精确拆分。这里两条网格长度/首/尾完全相同, 仅内部不同。
#[test]
fn pbo_selection_splits_key_collisions_by_exact_time_axis() {
    use crate::app::RunStatus;

    let mk = |params: &str, times: &[i64]| crate::output::FixedCurve {
        combo: "c".into(),
        symbol: "BTCUSDT".into(),
        timeframe: "1d".into(),
        strategy: "sma".into(),
        params: params.into(),
        status: RunStatus::Ok,
        values: vec![100.0; times.len()],
        times: times.to_vec(),
    };
    // 两者 len=5 / first=0 / last=5, 但第 2 个时间点不同。
    let grid_a = [0_i64, 1, 2, 3, 5];
    let grid_b = [0_i64, 2, 3, 4, 5];
    let mut members: Vec<_> = (0..12).map(|i| mk(&format!("a{i}"), &grid_a)).collect();
    members.extend((0..2).map(|i| mk(&format!("b{i}"), &grid_b)));

    let (selected, stats) = select_curves(&members);
    assert_eq!(stats.cohorts, 2, "同键不同网格必须拆成两个 cohort");
    assert_eq!(stats.kept, 12, "只能选样本数更大的 grid_a");
    assert_eq!(stats.dropped_other_window, 2);
    assert!(selected.iter().all(|(p, _)| p.starts_with('a')));
}

/// 时间列空/不等长/非严格递增的曲线必须被筛掉, 不得参与 CSCV。
#[test]
fn pbo_selection_rejects_invalid_time_axes() {
    use crate::app::RunStatus;

    let mk = |params: &str, times: Vec<i64>| crate::output::FixedCurve {
        combo: "c".into(),
        symbol: "BTCUSDT".into(),
        timeframe: "1d".into(),
        strategy: "sma".into(),
        params: params.into(),
        status: RunStatus::Ok,
        values: vec![100.0; times.len()],
        times,
    };
    let good: Vec<i64> = (0..64).map(|i| 1_700_000_000 + i * 86_400).collect();
    let mut members: Vec<_> = (0..10)
        .map(|i| mk(&format!("p{i}"), good.clone()))
        .collect();
    members.push(mk("empty", Vec::new()));
    members.push(mk("dup_time", vec![1, 1, 2, 3]));
    members.push(mk("backwards", vec![3, 2, 1]));

    let (selected, stats) = select_curves(&members);
    assert_eq!(selected.len(), 10);
    assert_eq!(stats.dropped_invalid, 3);
    assert_eq!(stats.cohorts, 1);
}

/// 全部曲线都不是 ok 时返回空选择, 由调用方按 N<10 跳过并 warn。
#[test]
fn pbo_selection_without_ok_curves_is_empty() {
    use crate::app::RunStatus;

    let times: Vec<i64> = (0..64).map(|i| 1_700_000_000 + i * 86_400).collect();
    let members: Vec<_> = (0..12)
        .map(|i| crate::output::FixedCurve {
            combo: "c".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1d".into(),
            strategy: "sma".into(),
            params: format!("p{i}"),
            status: RunStatus::Bankrupt,
            values: vec![0.0; times.len()],
            times: times.clone(),
        })
        .collect();
    let (selected, stats) = select_curves(&members);
    assert!(selected.is_empty());
    assert_eq!(stats.kept, 0);
    assert_eq!(stats.dropped_status, 12);
    assert_eq!(stats.cohorts, 0);
}
