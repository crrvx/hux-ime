#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成音反查金样（⑧-1）：参照 `feat/reverse-lookup` 尖端 pin 的 Lua 核心 + 系统 librime + librime-lua。
# 该 pin **已包含主干**（`abad411` 等主干提交都在其祖先链上），故金样即 pin 树本身，
# 不再需要「分支 pin + 主干 pin 本地合并」；`PIN` 取分支尖端（当前 92a0b54）。
# 仍保留原合并护栏的精神：参与生成的文件只要与声明 pin 不符（工作区被改动、或有人
# 重新引入本地合并），就**显式失败**，绝不静默产出与声明 pin 不符的金样。
#
# 用法：tools/generators/gen_sound_to_char_shape_golden.sh [输出文件]
#   REF  参照仓库本地检出（默认仓库内 external/tiger-sentense-rime，已 gitignore）
#   REF_URL  写入金样头部的参照仓库线上地址（默认 https://github.com/lvyww/tiger-sentense-rime）
#   PIN  参照提交（默认 92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c = feat/reverse-lookup 尖端，含主干）
#   CASES 用例文件（默认 tools/cases/sound_to_char_shape_cases.txt）
#
# 夹具（goldens/sound_to_char_shape/）：小 PY_c 词典 + 合成码表 + symbols.yaml（pin 同文件）；
# 同一夹具供 Rust 重放（`tiger_sentence.pinyin.bin` 由 tools/generators/gen_pinyin_index.py 生成）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本），见 goldens/README.md。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF="${REF:-$ROOT/external/tiger-sentense-rime}"
REF_URL="${REF_URL:-https://github.com/lvyww/tiger-sentense-rime}"
PIN="${PIN:-92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c}"
OUT="${1:-$ROOT/goldens/sound_to_char_shape.tsv.gz}"
CASES="${CASES:-$ROOT/tools/cases/sound_to_char_shape_cases.txt}"
FIXTURE="$ROOT/goldens/sound_to_char_shape"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-pinyin-XXXXXX")"
WT="$WORK/ref"
trap 'git -C "$REF" worktree remove --force "$WT" 2>/dev/null || true; rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
mkdir -p "$user/lua" "$shared"

# pin 树（detached worktree，不触碰参照仓库的分支/引用）。
git -C "$REF" worktree add --detach --force "$WT" "$PIN" >/dev/null
# 护栏：生成输入必须**就是**该 pin——HEAD 不是 pin（例如有人重新引入本地合并）
# 或工作区不干净（参与生成的文件被改动/冲突残留）时显式失败。
if [ "$(git -C "$WT" rev-parse HEAD)" != "$PIN" ] ||
    [ -n "$(git -C "$WT" status --porcelain)" ]; then
    echo "生成失败：$WT 不是干净的 $PIN（本地合并或工作区改动会产出与声明 pin 不符的金样）" >&2
    git -C "$WT" status --porcelain >&2 || true
    exit 1
fi

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

# 音反查索引夹具：由小 PY_c 生成（Rust 重放用）。
python3 "$ROOT/tools/generators/gen_pinyin_index.py" \
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
g++ -std=c++17 -O2 "$ROOT/tools/probes/rime_sequence_probe.cpp" -lrime -ldl -o "$WORK/probe"

lua_sha="$(sha256sum "$WT/lua/tiger_sentence.lua" | cut -d' ' -f1)"
pyc_sha="$(sha256sum "$FIXTURE/PY_c.dict.yaml" | cut -d' ' -f1)"
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# pinyin lookup golden (⑧-1)\n'
    printf '# reference: %s @ %s (feat/reverse-lookup tip; includes main)\n' "$REF_URL" "$PIN"
    printf '# tiger_sentence.lua sha256: %s\n' "$lua_sha"
    printf '# PY_c.dict.yaml sha256: %s\n' "$pyc_sha"
    printf '# librime: %s; plugin: %s\n' "${librime_version:-unknown}" "$plugin"
    LD_LIBRARY_PATH="$WORK${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$WORK/probe" "$user" "$shared" "$plugin" "$CASES"
} | gzip -9 > "$OUT.tmp.$$"

# 写库前断言：至少产出 1 个用例，避免空/全注释 CASES 把入库金样静默覆盖成只剩头部。
if ! gzip -cd "$OUT.tmp.$$" | grep -q '^case'; then
    rm -f "$OUT.tmp.$$"
    echo "生成失败：$OUT 不含任何用例（检查 CASES 是否为空或全为注释）" >&2
    exit 1
fi
mv "$OUT.tmp.$$" "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | wc -l) lines)"
