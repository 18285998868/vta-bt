//! 构建期计算**源码内容哈希**, 供运行期缓存指纹使用。
//!
//! # 为什么需要
//!
//! 缓存指纹(`app::params_fingerprint`)覆盖: 行情内容、资金、窗口、执行口径、
//! 产出开关、参数串。它**不覆盖实现** —— 于是"改完策略代码再重跑"会
//! 全部命中旧缓存, 报告里仍是旧代码的结果, 且**无任何提示**。
//! 这是研究工具里最典型的"结论悄悄过期"失效模式: 修完一个 SMA 策略的 bug
//! 后重跑, 以为拿到了新结果, 其实拿到的是修复前的。
//!
//! 本脚本对**本 crate 的 `src/`** 与**三个 path 依赖的 `src/`**
//! (`vector-ta` / `binance-data-reader` / `dukascopy-data-reader`) 做内容哈希
//! —— 后三者同样影响结果 (指标实现住在 `vector-ta` 里, 数据读取住在两个
//! reader 里), 只哈希本 crate 是不够的。
//!
//! # 为什么用内容哈希而非 git commit
//!
//! 本仓库是 monorepo, `git rev-parse HEAD` 会把**整个仓库**(含无关的兄弟
//! crate)的提交算进来 —— 改一个与回测无关的 crate 也会让缓存全量失效。
//! 非 git 检出(vendored / 打包分发)时 `git` 不可用。
//! 工作区有未提交改动时 commit hash **不变** ⇒ 恰恰是"刚改完代码"这个
//! 最需要失效的时刻, 缓存**不会**失效。
//!
//! 内容哈希对上述三种情况都正确。
//!
//! # 失效时机
//!
//! 哈希经 `cargo:rustc-env=VTA_SOURCE_HASH=…` 注入, 运行期用
//! `env!("VTA_SOURCE_HASH")` 读取(编译期常量)。因此:
//! 源码一变 ⇒ 哈希变 ⇒ 指纹变 ⇒ 旧缓存**必然**不命中;
//! 同时本脚本对每个参与哈希的文件发 `cargo:rerun-if-changed`,
//! 保证 cargo **重建**本 crate(否则 `env!` 里的常量还是旧值)。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// FNV-1a 的初始偏移与质数(与运行期 `app::fnv_bytes` 同一套算法)。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 哈希算法版本盐: 算法本身改动时也要让哈希变化。
const SALT: &str = "vta-source-hash-v1";

/// 参与哈希的**源码根目录**(相对 `CARGO_MANIFEST_DIR`)。
///
/// 与 `Cargo.toml` 的 `[workspace.dependencies]` 一一对应:
/// - `vector-ta` / `binance-data-reader` / `dukascopy-data-reader` 是 path
///   依赖, 它们的 `src/` 里住着指标实现与数据读取逻辑, **直接影响回测数值**。
///
/// 目录不存在时**跳过并告警**(不阻断构建): 例如把本 crate 单独 vendored
/// 出去时 path 依赖可能缺失 —— 此时哈希只覆盖本 crate, 退化为"部分覆盖",
/// 但仍好过完全没有。
const HASHED_ROOTS: &[&str] = &[
    "src",
    "../libs/VectorTA/src",
    "../libs/binance-data-reader/src",
    "../libs/dukascopy-data-reader/src",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // 不用 `expect`: 本 crate 的 lint 门禁对 `expect_used` 是 deny, 而
    // build script 也不例外。拿不到 `CARGO_MANIFEST_DIR` 时降级为哨兵值,
    // 不阻断构建。
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            println!("cargo:warning=vta: CARGO_MANIFEST_DIR 未设置, 源码哈希不可用");
            println!("cargo:rustc-env=VTA_SOURCE_HASH=unknown");
            return;
        }
    };

    let mut h = FNV_OFFSET;
    h = fnv_bytes(h, SALT.as_bytes());

    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for root in HASHED_ROOTS {
        let dir = manifest_dir.join(root);
        match collect_rs_files(&dir) {
            Ok(fs) if !fs.is_empty() => files.extend(fs),
            Ok(_) => {
                println!(
                    "cargo:warning=vta: 源码目录 {} 不存在或无 .rs 文件, 该部分**未**计入源码哈希",
                    dir.display()
                );
            }
            Err(e) => {
                println!(
                    "cargo:warning=vta: 读取源码目录 {} 失败({e}), 该部分**未**计入源码哈希",
                    dir.display()
                );
            }
        }
    }

    if files.is_empty() {
        // 绝不能让 `env!` 取不到值(那会编译失败): 给一个明确的哨兵。
        println!("cargo:warning=vta: 未找到任何参与哈希的源文件, 缓存指纹的代码版本维度失效");
        println!("cargo:rustc-env=VTA_SOURCE_HASH=unknown");
        return;
    }

    for f in &files {
        // 必须逐文件发 rerun-if-changed: 只要发出一条, cargo 就**停止**默认的
        // "包内任何文件变化都重跑 build script" 行为, 故必须显式列全。
        println!("cargo:rerun-if-changed={}", f.display());
    }

    for f in &files {
        // 混入**相对路径**: 两个不同目录下的同名文件(`src/config.rs` 与
        // `../libs/VectorTA/src/config.rs`)不得产生相同的字节流。
        let rel = f
            .strip_prefix(&manifest_dir)
            .unwrap_or(f.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        h = fnv_bytes(h, rel.as_bytes());
        h = fnv_bytes(h, &[0xff]);
        match std::fs::read(f) {
            Ok(bytes) => h = fnv_bytes(h, &bytes),
            Err(e) => {
                println!("cargo:warning=vta: 读取 {} 失败({e}), 已跳过", f.display());
            }
        }
        h = fnv_bytes(h, &[0xff]);
    }

    println!("cargo:rustc-env=VTA_SOURCE_HASH={h:016x}");
}

/// 递归收集目录下的全部 `.rs` 文件(排序后返回, 保证顺序稳定)。
fn collect_rs_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&d)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 把一段字节混入 FNV-1a。
fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}
