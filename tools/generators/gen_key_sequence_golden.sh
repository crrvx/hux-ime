#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成键序列金样（2c）：pin 版参照 Lua 核心 + 系统 librime + librime-lua。
#
# 用法：tools/generators/gen_key_sequence_golden.sh [输出文件]
#   REF  参照仓库本地检出（默认仓库内 external/tiger-sentense-rime，已 gitignore）
#   REF_URL  写入金样头部的参照仓库线上地址（默认 https://github.com/lvyww/tiger-sentense-rime）
#   PIN  参照固定提交（默认 8b615235c17c858e1eca8f1a41fbc74e202f8bbe，与入库金样一致；见 goldens/README.md）
#   CASES 用例文件（默认 tools/cases/key_sequence_cases.txt；可指向临时用例做探索）
#
# 依赖：git、g++、python3、系统 librime（rime_api.h + librime-lua.so）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本），见 goldens/README.md。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF="${REF:-$ROOT/external/tiger-sentense-rime}"
REF_URL="${REF_URL:-https://github.com/lvyww/tiger-sentense-rime}"
PIN="${PIN:-8b615235c17c858e1eca8f1a41fbc74e202f8bbe}"
OUT="${1:-$ROOT/goldens/key_sequence.tsv.gz}"
CASES="${CASES:-$ROOT/tools/cases/key_sequence_cases.txt}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-keyseq-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
mkdir -p "$user/lua" "$shared"

# pin 版 Lua 核心与 schema（保证与已入库金样同一参照修订）。
for name in tiger_sentence.lua tiger_sentence_learning.lua tiger_sentence_ngram.lua \
    tiger_sentence_cache.lua tiger_sentence_lexical.lua; do
    git -C "$REF" show "$PIN:lua/$name" > "$user/lua/$name"
done
git -C "$REF" show "$PIN:rime.lua" > "$user/rime.lua"
git -C "$REF" show "$PIN:tiger_sentence.schema.yaml" > "$user/tiger_sentence.schema.yaml"
git -C "$REF" show "$PIN:tiger_sentence_ascii.schema.yaml" > "$user/tiger_sentence_ascii.schema.yaml"
git -C "$REF" show "$PIN:symbols.yaml" > "$user/symbols.yaml"

# 合成小码表（与参照集成测试同构：单字/词组、可控重码与 Tab 翻页；
# 另含 1 键码 + 数字结尾文本，覆盖空码自动上屏（`try_empty_code_commit`）路径）。
# 同一份数据入库到 goldens/key_sequence/，供 Rust 重放侧加载。
# 标点表 symbols.yaml 同步入库（与探针 user 目录同一来源），供标点用例重放。
mkdir -p "$ROOT/goldens/key_sequence"
git -C "$REF" show "$PIN:symbols.yaml" > "$ROOT/goldens/key_sequence/symbols.yaml"
python3 - "$user" "$ROOT/goldens/key_sequence" <<'PY'
import pathlib
import sys
user = pathlib.Path(sys.argv[1])
golden_dir = pathlib.Path(sys.argv[2])
table = ["刘\tvp", "甲\tab", "乙\tab", "一\tcd", "第7\tz"]
table += [f"{chr(0x4E00 + i)}\tja" for i in range(22)]
content = "\n".join(table) + "\n"
(golden_dir / "tiger_sentence.codes.txt").write_text(content, encoding="utf-8")
(user / "tiger_sentence.codes.txt").write_text(content, encoding="utf-8")
(user / "tiger_sentence.custom.yaml").write_text(
    "patch:\n  tiger_sentence/high_freq_limit: 0\n"
    "  tiger_sentence/tab_learning: false\n", encoding="utf-8")
PY

# 最小共享数据（与参照集成测试同构）。
cat > "$shared/default.yaml" <<'YAML'
config_version: "1.0"
schema_list:
  - schema: tiger_sentence
menu:
  page_size: 5
recognizer:
  patterns: {}
YAML

# 探针（系统 librime；librime-lua 插件显式加载）。
plugin="${LUA_PLUGIN:-/usr/lib/rime-plugins/librime-lua.so}"
test -f "$plugin"
g++ -std=c++17 -O2 "$ROOT/tools/probes/rime_sequence_probe.cpp" -lrime -ldl -o "$WORK/probe"

lua_sha="$(git -C "$REF" show "$PIN:lua/tiger_sentence.lua" | sha256sum | cut -d' ' -f1)"
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# key_sequence golden (2c)\n'
    printf '# reference: %s @ %s\n' "$REF_URL" "$PIN"
    printf '# tiger_sentence.lua sha256: %s\n' "$lua_sha"
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
