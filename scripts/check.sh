#!/usr/bin/env bash
# OS: __CachyOS__
# TIME: 2026-11-13
# DOC: vta 本地与 CI 共用的唯一门禁入口
# LOGIC:
#   STEP 1: 格式化检查(只检查 workspace 成员, 不碰仓库外的 path 依赖)
#   STEP 2: cargo check --all-targets
#   STEP 3: clippy(all-features / no-default-features, -D warnings)
#   STEP 4: nextest + doctest(no-default/all-features 至少各构建一次)
#   STEP 5: rustdoc(-D warnings)
#   STEP 6: cargo deny(许可证/来源/公告) + cargo audit(RustSec)
# USAGE:
#   ./scripts/check.sh [--ci]
#
# Q:
#   Q1: `cargo fmt --all` 只格式化 workspace 成员; `binance-core` / `dukascopy-core`
#       是仓库外的 path 依赖, 不是成员, 不会被触碰。C2 起策略模块由宏声明,
#       rustfmt 无法沿 mod 树发现, 故 STEP 1 另有一次 `find ... rustfmt --check`。
#   Q2: 工具缺失时直接失败, 不在门禁脚本里自动安装全局工具。

# 直接执行时启用严格模式; 被 source 时不改调用者的 shell 选项
if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
    set -Eeuo pipefail
fi

_self_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
_repo_root=$(cd -- "${_self_dir}/.." && pwd -P)
cd -- "$_repo_root"

info() { printf '%s\n' "$*" >&2; }
die() {
    printf '%s: %s\n' "${0##*/}" "$*" >&2
    exit 1
}
run() {
    info "==> $*"
    "$@"
}

case ${1-} in
    --ci)
        export CARGO_INCREMENTAL=0
        info "CI 模式: CARGO_INCREMENTAL=0"
        ;;
    '') ;;
    *) die "用法: ${0##*/} [--ci]" ;;
esac

run cargo fmt --all -- --check
# `cargo fmt` 只沿 crate 根解析 mod 树; C2 起策略模块由 `strategy_table!`
# 宏声明, rustfmt 看不到, 85 个策略文件会静默脱离格式门禁。这里显式全查。
run bash -c 'find crates -name "*.rs" -not -path "*/target/*" -print0 | xargs -0 rustfmt --edition 2024 --check'
run cargo check --workspace --all-targets --locked
run cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
run cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings

if command -v cargo-nextest >/dev/null 2>&1; then
    run cargo nextest run --workspace --all-features --locked
else
    info "cargo-nextest 未安装, 回退到 cargo test"
    run cargo test --workspace --all-features --locked
fi

# nextest 不跑 doctest, 必须单独跑一次。
run cargo test --workspace --doc --all-features --locked
run env RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked
run cargo deny check
run cargo audit

info "全部检查通过"
