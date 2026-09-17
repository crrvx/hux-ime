#!/usr/bin/env bash
# 生成键金样（只需要系统 librime；另需 pin 版 key_table.cc 以取键值清单）。
#   tools/gen_key_golden.sh <librime-src>
#
# <librime-src>：含 src/rime/key_table.cc 的目录；单文件下载即可，无需克隆：
#   mkdir -p external/librime/src/rime
#   curl -fsSL -o external/librime/src/rime/key_table.cc \
#     https://raw.githubusercontent.com/rime/librime/33e78140250125871856cdc5b42ddc6a5fcd3cd4/src/rime/key_table.cc
#
# 脚本会校验该文件 sha256 与 crates/hux-core/src/key_table.rs 头部记录一致。
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src=${1:?usage: gen_key_golden.sh <librime-src>}
key_table_cc="$src/src/rime/key_table.cc"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# 校验源码 sha：必须与入库 key_table.rs 头部记录的 pin 文件一致（防键值清单漂移）。
if [ ! -f "$key_table_cc" ]; then
    echo "缺少 $key_table_cc（单文件下载命令见本脚本头部注释）" >&2
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
python3 "$root/tools/gen_key_table.py" \
    --source "$key_table_cc" \
    --out /dev/null --keyvals-out "$work/keyvals.txt" >/dev/null
printf '%s\n' 1 255 4660 >> "$work/keyvals.txt" # 0x1, 0xff, 0x1234

g++ -std=c++17 -O2 "$root/tools/key_probe.cpp" -lrime -o "$work/key_probe"
"$work/key_probe" "$work/keyvals.txt" "$root/tools/key_cases.txt" > "$work/key.tsv"
gzip -9 -n -c "$work/key.tsv" > "$root/goldens/key.tsv.gz"
wc -l "$work/key.tsv"
sha256sum "$root/goldens/key.tsv.gz"
