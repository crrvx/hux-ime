#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成键序列金样（2c）：pin 版参照 Lua 核心 + 系统 librime + librime-lua。
#
# 用法：tools/generators/gen_key_sequence_golden.sh [输出文件]
#   REF  参照仓库本地检出（默认仓库内 _external/tiger-sentense-rime，已 gitignore）
#   REF_URL  写入金样头部的参照仓库线上地址（默认 https://github.com/lvyww/tiger-sentense-rime）
#   PIN  参照固定提交（默认 abad411750f79cfca750985fa266689b5d9b865f＝主干 pin，与入库金样一致；
#        音反查金样取「反查分支尖端」92a0b54，见 gen_sound_to_char_shape_golden.sh）
#   CASES 用例文件（默认 tools/cases/key_sequence_cases.txt；可指向临时用例做探索）
#
# 依赖：git、g++、python3、系统 librime（rime_api.h + librime-lua.so）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本）——需本地按 `tools/generators/` 手动重生成。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF="${REF:-$ROOT/_external/tiger-sentense-rime}"
REF_URL="${REF_URL:-https://github.com/lvyww/tiger-sentense-rime}"
PIN="${PIN:-abad411750f79cfca750985fa266689b5d9b865f}"
OUT="${1:-$ROOT/goldens/key_sequence.tsv.gz}"
CASES="${CASES:-$ROOT/tools/cases/key_sequence_cases.txt}"
FIXTURE="$ROOT/goldens/key_sequence"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-keyseq-XXXXXX")"
trap 'rm -f "$OUT.tmp.$$"; rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
stage="$WORK/stage"
mkdir -p "$user/lua" "$shared" "$stage"

# 夹具护栏：入库夹具（goldens/key_sequence/）不得被生成器当副作用重写。
# 只做「逐字节比对」这一件事（不删传入文件——它可能是 pin 工作区里的真实文件）：
# 一致才继续，入库文件保持原样不落盘；不一致即失败，并区分两种成因：
# 上游 pin 变化（须同步更新金样与夹具）或夹具漂移（应还原）。
# 用法：guard_fixture <本次生成的临时产物> <入库文件> <说明>
guard_fixture() {
    local staged="$1" committed="$2" what="$3"
    if [ ! -f "$committed" ]; then
        echo "生成失败：入库夹具缺失：$committed（$what）" >&2
        exit 1
    fi
    if ! cmp -s "$staged" "$committed"; then
        echo "生成失败：$what 与入库夹具不一致（护栏拦下，未写入任何入库文件）" >&2
        echo "  入库：$committed  sha256 $(sha256sum "$committed" | cut -d' ' -f1)" >&2
        echo "  本次：$staged  sha256 $(sha256sum "$staged" | cut -d' ' -f1)" >&2
        echo "  成因二选一：①参照 pin $PIN 的对应源文件已变（上游推进）——须同步更新金样与夹具，" >&2
        echo "  不能由生成器静默覆盖；②入库夹具被本地改动（夹具漂移）——应还原夹具。" >&2
        exit 1
    fi
    echo "夹具一致（入库文件未改写）：$what -> $committed" >&2
}

# pin 版 Lua 核心与 schema（保证与已入库金样同一参照修订）。
for name in tiger_sentence.lua tiger_sentence_learning.lua tiger_sentence_ngram.lua \
    tiger_sentence_cache.lua tiger_sentence_lexical.lua; do
    git -C "$REF" show "$PIN:lua/$name" > "$user/lua/$name"
done
git -C "$REF" show "$PIN:rime.lua" > "$user/rime.lua"
git -C "$REF" show "$PIN:tiger_sentence.schema.yaml" > "$user/tiger_sentence.schema.yaml"
git -C "$REF" show "$PIN:tiger_sentence_ascii.schema.yaml" > "$user/tiger_sentence_ascii.schema.yaml"

# 标点表 symbols.yaml：探针输入与入库夹具（goldens/key_sequence/symbols.yaml）必须同为
# pin $PIN 的同一文件（参照集成测试同源）；故先比对、探针再用入库文件，绝不改写它。
git -C "$REF" show "$PIN:symbols.yaml" > "$stage/symbols.yaml"
guard_fixture "$stage/symbols.yaml" "$FIXTURE/symbols.yaml" "标点表 symbols.yaml（pin $PIN）"
cp "$FIXTURE/symbols.yaml" "$user/symbols.yaml"

# 合成小码表（与参照集成测试同构：单字/词组、可控重码与 Tab 翻页；
# 另含 1 键码 + 数字结尾文本，覆盖空码自动上屏（`try_empty_code_commit`）路径）。
# 同一份数据入库到 goldens/key_sequence/，供 Rust 重放侧加载（同样先比对、后使用入库文件）。
python3 - "$user" "$stage" <<'PY'
import pathlib
import sys
user = pathlib.Path(sys.argv[1])
stage = pathlib.Path(sys.argv[2])
table = ["刘\tvp", "甲\tab", "乙\tab", "一\tcd", "第7\tz"]
table += [f"{chr(0x4E00 + i)}\tja" for i in range(22)]
content = "\n".join(table) + "\n"
(stage / "tiger_sentence.codes.txt").write_text(content, encoding="utf-8")
(user / "tiger_sentence.custom.yaml").write_text(
    "patch:\n  tiger_sentence/high_freq_limit: 0\n"
    "  tiger_sentence/tab_learning: false\n", encoding="utf-8")
PY
guard_fixture "$stage/tiger_sentence.codes.txt" "$FIXTURE/tiger_sentence.codes.txt" \
    "合成码表 tiger_sentence.codes.txt"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"

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
# 插件缺失时显式报错：`set -e` 下裸 `test -f` 会静默退出，无从诊断。
plugin="${LUA_PLUGIN:-/usr/lib/rime-plugins/librime-lua.so}"
if [ ! -f "$plugin" ]; then
    echo "生成失败：缺少 librime-lua 插件：$plugin（可用 LUA_PLUGIN 覆盖）" >&2
    exit 1
fi
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
