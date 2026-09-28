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
# 夹具（goldens/key_sequence/）不得被生成器当副作用重写；护栏与最小共享数据见
# tools/generators/lib/golden_fixture.sh。
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
# shellcheck source=tools/generators/lib/golden_fixture.sh
source "$ROOT/tools/generators/lib/golden_fixture.sh"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-keyseq-XXXXXX")"
trap 'rm -f "$OUT.tmp.$$"; rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
stage="$WORK/stage"
mkdir -p "$user/lua" "$shared" "$stage"

# pin 版 Lua 核心与 schema（保证与已入库金样同一参照修订）。
copy_lua_modules "$user"
pin_show rime.lua "$user/rime.lua"
pin_show tiger_sentence.schema.yaml "$user/tiger_sentence.schema.yaml"
pin_show tiger_sentence_ascii.schema.yaml "$user/tiger_sentence_ascii.schema.yaml"

# 标点表 symbols.yaml：探针输入与入库夹具（goldens/key_sequence/symbols.yaml）必须同为
# pin $PIN 的同一文件（参照集成测试同源）；故先比对、探针再用入库文件，绝不改写它。
pin_show symbols.yaml "$stage/symbols.yaml"
guard_fixture "$stage/symbols.yaml" "$FIXTURE/symbols.yaml" "标点表 symbols.yaml（pin $PIN）"
cp "$FIXTURE/symbols.yaml" "$user/symbols.yaml"

# 合成小码表（与参照集成测试同构：单字/词组、可控重码与 Tab 翻页；
# 另含 1 键码 + 数字结尾文本，覆盖空码自动上屏（`try_empty_code_commit`）路径）。
# 同一份数据入库到 goldens/key_sequence/，供 Rust 重放侧加载（同样先比对、后使用入库文件）。
synth_code_table "$stage" "$user" false
guard_fixture "$stage/tiger_sentence.codes.txt" "$FIXTURE/tiger_sentence.codes.txt" \
    "合成码表 tiger_sentence.codes.txt"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"

default_yaml "$shared/default.yaml"

# 探针（系统 librime；librime-lua 插件显式加载）。
plugin="$(require_lua_plugin)"
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
} > "$WORK/golden.tsv"

# 写库前断言 + 原子写库：至少 1 个用例，避免空/全注释 CASES 把入库金样静默覆盖成只剩头部。
golden_require "$WORK/golden.tsv" '^case' "金样不含任何用例"
write_golden "$WORK/golden.tsv" "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | wc -l) lines)"
