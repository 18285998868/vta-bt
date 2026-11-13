#!/usr/bin/env bash
# OS: __CachyOS__
# TIME: 2026-11-13
# DOC: vta 本地与 CI 共用的唯一门禁入口
# LOGIC:
#   STEP 1: 格式化检查(只检查本 package, 不碰仓库外的 path 依赖)
#   STEP 2: cargo check --all-targets
#   STEP 3: clippy(all-features / no-default-features, -D warnings)
#   STEP 4: nextest + doctest(no-default/all-features 至少各构建一次)
#   STEP 5: rustdoc(-D warnings)
#   STEP 6: cargo deny(许可证/来源/公告) + cargo audit(RustSec)
# USAGE:
#   ./scripts/check.sh [--ci]
#
# Q:
#   Q1: `cargo fmt --all` 会顺带格式化仓库外的 path 依赖(binance/dukascopy reader),
#       故这里用 `-p vta` 限定范围; 外部仓库是否有 fmt 问题与本项目无关。
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

run cargo fmt -p vta -- --check
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
