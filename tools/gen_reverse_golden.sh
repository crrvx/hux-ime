#!/usr/bin/env bash
# 生成反查金样（⑧-1）：参照分支（含 PY_c 反查）的 Lua 核心 + 系统 librime + librime-lua。
#
# 用法：tools/gen_reverse_golden.sh [输出文件]
#   REF  参照仓库路径（默认与仓库同级的 ../tiger-sentense-rime）
#   PIN  参照固定提交（默认 898579f833df53f1dec5639d56e685751a8a7f71，含 PY_c 与反查接线）
#   CASES 用例文件（默认 tools/key_sequence_reverse_cases.txt）
#
# 夹具（goldens/reverse/）：小 PY_c 词典 + 合成码表 + symbols.yaml（pin 同文件）；
# 同一夹具供 Rust 重放（`tiger_sentence.reverse.bin` 由 tools/gen_reverse_index.py 生成）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本），见 goldens/README.md。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${REF:-$(cd "$ROOT/.." && pwd)/tiger-sentense-rime}"
PIN="${PIN:-898579f833df53f1dec5639d56e685751a8a7f71}"
OUT="${1:-$ROOT/goldens/reverse.tsv.gz}"
CASES="${CASES:-$ROOT/tools/key_sequence_reverse_cases.txt}"
FIXTURE="$ROOT/goldens/reverse"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-reverse-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
mkdir -p "$user/lua" "$shared"

for name in tiger_sentence.lua tiger_sentence_learning.lua tiger_sentence_ngram.lua \
    tiger_sentence_cache.lua tiger_sentence_lexical.lua; do
    git -C "$REF" show "$PIN:lua/$name" > "$user/lua/$name"
done
git -C "$REF" show "$PIN:rime.lua" > "$user/rime.lua"
git -C "$REF" show "$PIN:tiger_sentence.schema.yaml" > "$user/tiger_sentence.schema.yaml"
git -C "$REF" show "$PIN:symbols.yaml" > "$user/symbols.yaml"
git -C "$REF" show "$PIN:PY_c.schema.yaml" > "$user/PY_c.schema.yaml"

# 夹具（入库；探针与 Rust 重放共用）。
cp "$FIXTURE/PY_c.dict.yaml" "$user/PY_c.dict.yaml"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"
cp "$ROOT/goldens/key_sequence/symbols.yaml" "$FIXTURE/symbols.yaml"

# 反查索引夹具：由小 PY_c 生成（Rust 重放用）。
python3 "$ROOT/tools/gen_reverse_index.py" \
    --source "$FIXTURE/PY_c.dict.yaml" \
    --out "$FIXTURE/tiger_sentence.reverse.bin"

# 选项：与键序列夹具同构；页大小用 schema 默认（5，与 core host::DEFAULT_PAGE_SIZE 一致），
# 翻页用例据此覆盖第 2/3 页候选。
cat > "$user/tiger_sentence.custom.yaml" <<'YAML'
patch:
  tiger_sentence/high_freq_limit: 0
  tiger_sentence/tab_learning: false
YAML

cat > "$shared/default.yaml" <<'YAML'
config_version: "1.0"
schema_list:
  - schema: tiger_sentence
menu:
  page_size: 5
recognizer:
  patterns: {}
YAML

plugin="${LUA_PLUGIN:-/usr/lib/rime-plugins/librime-lua.so}"
test -f "$plugin"
g++ -std=c++17 -O2 "$ROOT/tools/rime_sequence_probe.cpp" -lrime -ldl -o "$WORK/probe"

lua_sha="$(git -C "$REF" show "$PIN:lua/tiger_sentence.lua" | sha256sum | cut -d' ' -f1)"
pyc_sha="$(sha256sum "$FIXTURE/PY_c.dict.yaml" | cut -d' ' -f1)"
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# reverse golden (⑧-1)\n'
    printf '# reference: %s @ %s\n' "$REF" "$PIN"
    printf '# tiger_sentence.lua sha256: %s\n' "$lua_sha"
    printf '# PY_c.dict.yaml sha256: %s\n' "$pyc_sha"
    printf '# librime: %s; plugin: %s\n' "${librime_version:-unknown}" "$plugin"
    LD_LIBRARY_PATH="$WORK${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$WORK/probe" "$user" "$shared" "$plugin" "$CASES"
} | gzip -9 > "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | wc -l) lines)"
