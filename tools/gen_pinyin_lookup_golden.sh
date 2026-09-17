#!/usr/bin/env bash
# 生成音查虎金样（⑧-1）：参照分支（含 PY_c 音查虎）的 Lua 核心 + 系统 librime + librime-lua。
# 参照态 = 分支提交 PIN 与主干提交 BASE 的**本地合并**（上游未合并该分支；合并保证
# 音查虎特性与主干修复（如自动上屏对齐）同时生效；生成器自建临时 worktree，可复现）。
#
# 用法：tools/gen_pinyin_lookup_golden.sh [输出文件]
#   REF  参照仓库本地检出（默认与仓库同级的 ../tiger-sentense-rime）
#   REF_URL  写入金样头部的参照仓库线上地址（默认 https://github.com/crrvx/tiger-sentense-rime）
#   PIN  音查虎分支提交（默认 898579f833df53f1dec5639d56e685751a8a7f71，含 PY_c 与音查虎接线）
#   BASE 主干提交（默认 8b615235c17c858e1eca8f1a41fbc74e202f8bbe；与 PIN 合并后生成）
#   CASES 用例文件（默认 tools/pinyin_lookup_cases.txt）
#
# 夹具（goldens/pinyin_lookup/）：小 PY_c 词典 + 合成码表 + symbols.yaml（pin 同文件）；
# 同一夹具供 Rust 重放（`tiger_sentence.pinyin.bin` 由 tools/gen_pinyin_index.py 生成）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本），见 goldens/README.md。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${REF:-$(cd "$ROOT/.." && pwd)/tiger-sentense-rime}"
REF_URL="${REF_URL:-https://github.com/crrvx/tiger-sentense-rime}"
PIN="${PIN:-898579f833df53f1dec5639d56e685751a8a7f71}"
BASE="${BASE:-8b615235c17c858e1eca8f1a41fbc74e202f8bbe}"
OUT="${1:-$ROOT/goldens/pinyin_lookup.tsv.gz}"
CASES="${CASES:-$ROOT/tools/pinyin_lookup_cases.txt}"
FIXTURE="$ROOT/goldens/pinyin_lookup"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-pinyin-XXXXXX")"
WT="$WORK/ref"
trap 'git -C "$REF" worktree remove --force "$WT" 2>/dev/null || true; rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
mkdir -p "$user/lua" "$shared"

# 本地合并（detached worktree，不触碰参照仓库的分支/引用）。
git -C "$REF" worktree add --detach --force "$WT" "$PIN" >/dev/null
git -C "$WT" -c user.name=golden -c user.email=golden@localhost \
    merge --no-ff --no-edit "$BASE" >/dev/null

for name in tiger_sentence.lua tiger_sentence_learning.lua tiger_sentence_ngram.lua \
    tiger_sentence_cache.lua tiger_sentence_lexical.lua; do
    cp "$WT/lua/$name" "$user/lua/$name"
done
cp "$WT/rime.lua" "$user/rime.lua"
cp "$WT/tiger_sentence.schema.yaml" "$user/tiger_sentence.schema.yaml"
cp "$WT/symbols.yaml" "$user/symbols.yaml"
cp "$WT/PY_c.schema.yaml" "$user/PY_c.schema.yaml"

# 夹具（入库；探针与 Rust 重放共用）。
cp "$FIXTURE/PY_c.dict.yaml" "$user/PY_c.dict.yaml"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"
cp "$ROOT/goldens/key_sequence/symbols.yaml" "$FIXTURE/symbols.yaml"

# 音查虎索引夹具：由小 PY_c 生成（Rust 重放用）。
python3 "$ROOT/tools/gen_pinyin_index.py" \
    --source "$FIXTURE/PY_c.dict.yaml" \
    --out "$FIXTURE/tiger_sentence.pinyin.bin"

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

lua_sha="$(sha256sum "$WT/lua/tiger_sentence.lua" | cut -d' ' -f1)"
pyc_sha="$(sha256sum "$FIXTURE/PY_c.dict.yaml" | cut -d' ' -f1)"
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# pinyin lookup golden (⑧-1)\n'
    printf '# reference: %s @ %s + %s (local merge)\n' "$REF_URL" "$PIN" "$BASE"
    printf '# tiger_sentence.lua sha256: %s\n' "$lua_sha"
    printf '# PY_c.dict.yaml sha256: %s\n' "$pyc_sha"
    printf '# librime: %s; plugin: %s\n' "${librime_version:-unknown}" "$plugin"
    LD_LIBRARY_PATH="$WORK${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$WORK/probe" "$user" "$shared" "$plugin" "$CASES"
} | gzip -9 > "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | wc -l) lines)"
