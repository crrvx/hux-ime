#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成键金样（只需要系统 librime；另需 pin 版 key_table.cc 以取键值清单）。
#   tools/generators/gen_key_golden.sh <librime-src>
#
# <librime-src>：含 src/rime/key_table.cc 的目录；单文件下载即可，无需克隆：
#   mkdir -p external/librime/src/rime
#   curl -fsSL -o external/librime/src/rime/key_table.cc \
#     https://raw.githubusercontent.com/rime/librime/33e78140250125871856cdc5b42ddc6a5fcd3cd4/src/rime/key_table.cc
#
# 脚本会校验该文件 sha256 与 crates/hux-core/src/key_table.rs 头部记录一致。
#
# 写库护栏（与另外两个探针生成器同构）：先写 `$OUT.tmp.$$`，断言至少 1 条 `name` 与 1 条 `parse`，
# 再原子 `mv`；`key_probe` 对「输入文件不可读」与「空输入」返回非零，故「输入缺失 ⇒ 入库金样被
# 静默覆盖成 32 行 modifier」这条路径不再成立。
# 金样头部记录参照 pin 与 key_table.cc sha256，由 `tools/checks/verify_golden_shas.py` 复核。
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:?usage: gen_key_golden.sh <librime-src>}
key_table_cc="$src/src/rime/key_table.cc"
cases="$root/tools/cases/key_cases.txt"
out="$root/goldens/key.tsv.gz"
# 参照 pin（键名表来源；与 CI 的 `LIBRIME_COMMIT` 同值，校验表由 CI 比对）。
librime_pin=33e78140250125871856cdc5b42ddc6a5fcd3cd4
librime_url=${LIBRIME_URL:-https://github.com/rime/librime}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# 校验源码 sha：必须与入库 key_table.rs 头部记录的 pin 文件一致（防键值清单漂移）。
if [ ! -f "$key_table_cc" ]; then
    echo "缺少 $key_table_cc（单文件下载命令见本脚本头部注释）" >&2
    exit 1
fi
if [ ! -f "$cases" ]; then
    echo "缺少用例文件 $cases" >&2
    exit 1
fi
expected=$(grep -m1 'key_table\.cc sha256' "$root/crates/hux-core/src/key_table.rs" | grep -oE '[0-9a-f]{64}')
actual=$(sha256sum "$key_table_cc" | cut -d' ' -f1)
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    echo "key_table.cc sha256 不匹配：期望 ${expected:-（未在 key_table.rs 中找到）}，实际 $actual" >&2
    echo "请按脚本头部注释下载 pin 版单文件后重试" >&2
    exit 1
fi

# 键值清单：全部表内键值 + 若干表外键值（验证 None 路径）。
python3 "$root/tools/generators/gen_key_table.py" \
    --source "$key_table_cc" \
    --out /dev/null --keyvals-out "$work/keyvals.txt" >/dev/null
printf '%s\n' 1 255 4660 >> "$work/keyvals.txt" # 0x1, 0xff, 0x1234

g++ -std=c++17 -O2 "$root/tools/probes/key_probe.cpp" -lrime -o "$work/key_probe"
# 头部与另外两个探针金样同构（`# reference: <url> @ <pin>` + 来源文件 sha256），
# 供 `tools/checks/verify_golden_shas.py` 与 README 的 pin / sha 表交叉核对。
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# key golden (librime key_probe)\n'
    printf '# reference: %s @ %s\n' "$librime_url" "$librime_pin"
    printf '# key_table.cc sha256: %s\n' "$actual"
    printf '# librime: %s; cases: tools/cases/key_cases.txt\n' "${librime_version:-unknown}"
    "$work/key_probe" "$work/keyvals.txt" "$cases"
} > "$work/key.tsv"

# 写库前断言：至少 1 条 `name` 与 1 条 `parse`（残缺输出不得覆盖入库金样）。
if ! grep -q '^name' "$work/key.tsv" || ! grep -q '^parse' "$work/key.tsv"; then
    echo "生成失败：$work/key.tsv 缺少 name / parse 记录（检查输入文件与 librime 探针）" >&2
    exit 1
fi
gzip -9 -n -c "$work/key.tsv" > "$out.tmp.$$"
mv "$out.tmp.$$" "$out"
wc -l "$work/key.tsv"
sha256sum "$out"
