#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 重跑全部 Lua 金样并与入库金样逐字节比对（CI 的 `golden` 与 `golden-lua-latest` 两作业共用入口）。
#
#   bash tools/generators/regen_goldens.sh [<参考检出>] [<金样目录>] [<临时目录>]
#
# 默认：参考检出 `_external/tiger-sentense-rime`、金样目录 `goldens`（只读）、临时目录 `$TMPDIR`。
# 退出码：0 全部一致；1 生成或比对失败；2 用法/依赖/路径错误。
#
# 入库金样只读：15 份产物一律写 <临时目录>，比对用 `gunzip -c <金样> | cmp - <产物>`
# （逐字节比 transcript 本身，不比 .gz 容器）。命令顺序与单份产物名单见下方两段。
set -euo pipefail

if [ $# -gt 3 ]; then
    echo "usage: $0 [<参考检出>] [<金样目录>] [<临时目录>]" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
reference=${1:-_external/tiger-sentense-rime}
goldens=${2:-goldens}
tmp=${3:-${TMPDIR:-/tmp}}
lua_bin=${LUA:-lua}

for tool in "$lua_bin" gunzip cmp; do
    command -v "$tool" >/dev/null 2>&1 || { echo "缺少命令：$tool" >&2; exit 2; }
done
[ -d "$reference" ] || { echo "参考检出不存在：$reference" >&2; exit 2; }
[ -d "$goldens" ] || { echo "金样目录不存在：$goldens" >&2; exit 2; }
mkdir -p "$tmp"
tmp=$(cd "$tmp" && pwd)
data=$(cd "$goldens" && pwd)
lexical_model="$root/data/tiger_sentence.lexical.bin"

gen() {
    echo "+ $lua_bin $*" >&2
    "$lua_bin" "$@"
}

# ---- 1. 重跑 15 份金样（顺序即 CI 的顺序）----------------------------------
gen tools/generators/gen_ngram_golden.lua --reference "$reference" \
    --model "$tmp/ngram_fixture.bin" --out "$tmp/ngram_fixture.tsv" --mode fixture
gen tools/generators/gen_lexicon_golden.lua --reference "$reference" \
    --data "$data/lexicon" --out "$tmp/lexicon.tsv" --mode present
gen tools/generators/gen_lexicon_golden.lua --reference "$reference" \
    --data "$tmp/no-such-dir" --out "$tmp/lexicon_missing.tsv" --mode missing
gen tools/generators/gen_lexicon_golden.lua --reference "$reference" \
    --data "$data/lexicon_variants" --out "$tmp/lexicon_variants.tsv" --mode present
gen tools/generators/gen_lexicon_golden.lua --reference "$reference" \
    --data "$data/lexicon_codes_only" --out "$tmp/lexicon_codes_only.tsv" --mode present
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --out "$tmp/decode.tsv"
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --model "$data/ngram_fixture.bin" \
    --lexical "$lexical_model" --out "$tmp/decode_model.tsv" --every 7
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --model "$data/ngram_fixture.bin" \
    --lexical "$lexical_model" \
    --out "$tmp/decode_rank_first.tsv" --every 7 --duplicate 0
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --out "$tmp/decode_evidence.tsv" --early-commit 1 --required 1
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --model "$data/ngram_fixture.bin" \
    --lexical "$lexical_model" \
    --out "$tmp/decode_evidence_model.tsv" --every 7 --early-commit 1 --required 1
gen tools/generators/gen_learning_golden.lua --reference "$reference" --out "$tmp/learning.tsv"
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --out "$tmp/decode_learning.tsv" --learning 1
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --model "$data/ngram_fixture.bin" \
    --lexical "$lexical_model" \
    --out "$tmp/decode_learning_model.tsv" --every 7 --learning 1
gen tools/generators/gen_decode_golden.lua --reference "$reference" \
    --data "$data/lexicon" --out "$tmp/decode_learning_evidence.tsv" \
    --early-commit 1 --required 1 --learning 1
gen tools/generators/gen_lexical_golden.lua --reference "$reference" \
    --model "$lexical_model" --out "$tmp/lexical.tsv"

# ---- 2. 与入库金样逐字节比对（同一份名单，缺一即失败）----------------------
failed=0
cmp_bin() {
    if cmp -s "$1" "$2"; then return 0; fi
    echo "DIFF $1 与 $2 不一致" >&2
    failed=1
}
cmp_bin "$goldens/ngram_fixture.bin" "$tmp/ngram_fixture.bin"
for name in ngram_fixture lexicon lexicon_missing lexicon_variants lexicon_codes_only \
    decode decode_model decode_rank_first decode_evidence decode_evidence_model \
    learning decode_learning decode_learning_model decode_learning_evidence lexical; do
    if gunzip -c "$goldens/$name.tsv.gz" | cmp -s - "$tmp/$name.tsv"; then continue; fi
    echo "DIFF $goldens/$name.tsv.gz 与 $tmp/$name.tsv 不一致（或产物缺失）" >&2
    failed=1
done
if [ "$failed" -ne 0 ]; then
    echo "regen_goldens: 有产物与入库金样不一致" >&2
    exit 1
fi
echo "regen_goldens: 15 份 Lua 金样与 $goldens 逐字节一致" >&2
