#!/usr/bin/env bash
# 生成键金样（需要 librime 源码头文件与系统 librime）：
#   tools/gen_key_golden.sh <librime-src>
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src=${1:?usage: gen_key_golden.sh <librime-src>}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# 键值清单：全部表内键值 + 若干表外键值（验证 None 路径）。
python3 "$root/tools/gen_key_table.py" \
    --source "$src/src/rime/key_table.cc" \
    --out /dev/null --keyvals-out "$work/keyvals.txt" >/dev/null
printf '%s\n' 1 255 4660 >> "$work/keyvals.txt" # 0x1, 0xff, 0x1234

g++ -std=c++17 -O2 "$root/tools/key_probe.cpp" -lrime -o "$work/key_probe"
"$work/key_probe" "$work/keyvals.txt" "$root/tools/key_cases.txt" > "$work/key.tsv"
gzip -9 -n -c "$work/key.tsv" > "$root/goldens/key.tsv.gz"
wc -l "$work/key.tsv"
sha256sum "$root/goldens/key.tsv.gz"
