#!/usr/bin/env bash
# OS: __CachyOS__
# TIME: 2026-11-15
# DOC: 重构的端到端行为冻结: release 冒烟 + parquet 逐值比较 + 幂等复跑
# LOGIC:
#   STEP 1: cargo build --release --locked
#   STEP 2: 用隔离输出目录跑一遍(release), 保存 result.parquet 为 run1
#   STEP 3: 同一目录再跑一遍(overwrite=false), 验证"不重复计算 + 结果一致"
#   STEP 4: --check 时与 reports/golden/baseline 逐值比较; --freeze 时写入基线
#   STEP 5: detail/ 逐个文件比较(文件名含参数哈希, 跨版本稳定)
# USAGE:
#   ./scripts/golden_run.sh --freeze   # 建立基线(只在行为变更被确认后执行)
#   ./scripts/golden_run.sh --check    # 与基线比较(默认)
#
# Q:
#   Q1: 为什么不比较 parquet 字节: 压缩与 writer 元数据会让相同内容产生不同字节。
#   Q2: 为什么不放 cargo test: 需要 32G 真实行情与 release 构建, 不属于单元测试。
#   Q3: 输出落在 reports/golden(已 gitignore), 基线不进版本库, 由使用者在
#       每台机器上 --freeze 一次。

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
    set -Eeuo pipefail
fi

_self_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
_repo_root=$(cd -- "${_self_dir}/.." && pwd -P)
cd -- "$_repo_root"

_mode=check
case ${1-} in
    --check) _mode=check ;;
    --freeze) _mode=freeze ;;
    '') ;;
    *) echo "用法: ${0##*/} [--check|--freeze]" >&2; exit 2 ;;
esac

_cfg_src=${2:-config.golden.toml}
_base_dir=reports/golden
_cur_dir=${_base_dir}/current
_base_file=${_base_dir}/baseline/result.parquet

info() { printf '%s\n' "$*" >&2; }
die() { info "${0##*/}: $*"; exit 1; }

[[ -f $_cfg_src ]] || die "配置不存在: $_cfg_src"
grep -q '^overwrite[[:space:]]*=' "$_cfg_src" || die "$_cfg_src 缺少 [output] overwrite 行"

# 生成隔离配置: 只改 [output] 段的 dir / overwrite, 其余原样保留。
make_cfg() {
    local dst=$1 outdir=$2 ow=$3
    awk -v out="$outdir" -v ow="$ow" '
        /^\[output\]/ { inout = 1; print; next }
        /^\[/ { inout = 0 }
        inout && /^dir[[:space:]]*=/ { print "dir = \"" out "\""; next }
        inout && /^overwrite[[:space:]]*=/ { print "overwrite = " ow; next }
        { print }
    ' "$_cfg_src" > "$dst"
}

run_once() {
    local cfg=$1 log=$2
    info "==> vta -c $cfg"
    RUST_LOG=info "./target/release/vta" -c "$cfg" 2>&1 | tee "$log"
}

info "==> cargo build --release --locked --bins --examples"
cargo build --release --locked --bins --examples

rm -rf "$_cur_dir"
mkdir -p "$_cur_dir" "$_base_dir/baseline"
_cfg1=$(mktemp "${TMPDIR:-/tmp}/vta-golden-XXXX.toml")
_cfg2=$(mktemp "${TMPDIR:-/tmp}/vta-golden-XXXX.toml")
trap 'rm -f "$_cfg1" "$_cfg2"' EXIT

make_cfg "$_cfg1" "$_cur_dir" true
make_cfg "$_cfg2" "$_cur_dir" false

run_once "$_cfg1" "$_cur_dir/run1.log"
[[ -f $_cur_dir/result.parquet ]] || die "首次运行未产出 result.parquet"
cp "$_cur_dir/result.parquet" "$_cur_dir/result.run1.parquet"
_rows=$(find "$_cur_dir/detail" -name '*.parquet' 2>/dev/null | wc -l)
info "run1: result.parquet + detail ${_rows} 个 parquet"

# 幂等: 同目录复跑(overwrite=false), 命中缓存仍须产出同样的结果。
run_once "$_cfg2" "$_cur_dir/run2.log"
info "==> 幂等比较: run1 vs run2"
./target/release/examples/parquet_diff \
    "$_cur_dir/result.run1.parquet" "$_cur_dir/result.parquet"

if [[ $_mode == freeze ]]; then
    cp "$_cur_dir/result.run1.parquet" "$_base_file"
    rm -rf "$_base_dir/baseline/detail"
    if [[ -d $_cur_dir/detail ]]; then
        cp -a "$_cur_dir/detail" "$_base_dir/baseline/detail"
    fi
    info "已写入基线: $_base_file (detail ${_rows} 个)"
    exit 0
fi

[[ -f $_base_file ]] || die "基线不存在: $_base_file —— 先跑一次 --freeze"

info "==> 与基线比较: result.parquet"
./target/release/examples/parquet_diff "$_base_file" "$_cur_dir/result.run1.parquet"

info "==> 与基线比较: detail/*.parquet"
_base_detail=${_base_dir}/baseline/detail
if [[ -d $_base_detail ]]; then
    _n=0
    while IFS= read -r f; do
        rel=${f#"$_cur_dir/detail/"}
        if [[ -f $_base_detail/$rel ]]; then
            ./target/release/examples/parquet_diff "$_base_detail/$rel" "$f" >/dev/null
            _n=$((_n + 1))
        else
            info "基线缺少 detail: $rel"
            exit 1
        fi
    done < <(find "$_cur_dir/detail" -name '*.parquet' | sort)
    info "detail 一致: ${_n} 个文件"
else
    info "基线没有 detail/(旧基线), 跳过 detail 比较"
fi

info "golden check 通过"
