#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成 Tab 锁路径金样：pin 版参照 Lua 核心 + 系统 librime +
# librime-lua，夹具 `tiger_sentence/tab_learning: true` ⇒ 参照的学习库**就绪**。
#
# 用法：tools/generators/gen_key_sequence_tab_golden.sh [输出文件]
#   REF  参照仓库本地检出（默认 `_external/tiger-sentense-rime`，已 gitignore）
#   PIN  参照固定提交（默认主干 pin `abad411750…`，与主金样一致）
#   CASES  用例文件（默认 `tools/cases/key_sequence_tab_cases.txt`）
#
# 与 `gen_key_sequence_golden.sh` 的差异：
#   ① 夹具目录为 `goldens/key_sequence_tab/`（多一份 `tiger_sentence.custom.yaml`，把
#      `tab_learning: true` 这一**条件本身**入库，供重放侧与读者核对）；
#   ② 夹具文件不再被静默重写：生成前用临时文件比对，内容不一致即失败并提示
#      「夹具漂移」还是「上游 pin 变化」；
#   ③ 写库前断言至少 1 个 `case` 且至少 1 步 `Tab` 被消费（否则说明夹具没生效）。
#
# 依赖：git、g++、python3、系统 librime（rime_api.h + librime-lua.so）。
# 金样不在 CI 重生成（探针依赖具体 librime/librime-lua 版本）——需本地按 `tools/generators/` 手动重生成。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF="${REF:-$ROOT/_external/tiger-sentense-rime}"
REF_URL="${REF_URL:-https://github.com/lvyww/tiger-sentense-rime}"
PIN="${PIN:-abad411750f79cfca750985fa266689b5d9b865f}"
OUT="${1:-$ROOT/goldens/key_sequence_tab.tsv.gz}"
CASES="${CASES:-$ROOT/tools/cases/key_sequence_tab_cases.txt}"
FIXTURE="$ROOT/goldens/key_sequence_tab"
# shellcheck source=tools/generators/lib/golden_fixture.sh
source "$ROOT/tools/generators/lib/golden_fixture.sh"

[ -f "$CASES" ] || { echo "缺少用例文件 $CASES" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-keyseq-tab-XXXXXX")"
trap 'rm -f "$OUT.tmp.$$"; rm -rf "$WORK"' EXIT
user="$WORK/user"
shared="$WORK/shared"
mkdir -p "$shared"

# pin 版 Lua 核心与 schema（保证与已入库金样同一参照修订）。
copy_lua_modules "$user"
pin_show rime.lua "$user/rime.lua"
pin_show tiger_sentence.schema.yaml "$user/tiger_sentence.schema.yaml"
pin_show tiger_sentence_ascii.schema.yaml "$user/tiger_sentence_ascii.schema.yaml"

# ---- 夹具：入库内容为唯一来源（漂移即失败，不静默重写）--------------------------
pin_show symbols.yaml "$WORK/symbols.yaml"
synth_code_table "$WORK" "$WORK" true
guard_fixture "$WORK/symbols.yaml" "$FIXTURE/symbols.yaml" "标点表 symbols.yaml（pin $PIN）"
guard_fixture "$WORK/tiger_sentence.codes.txt" "$FIXTURE/tiger_sentence.codes.txt" \
    "合成码表 tiger_sentence.codes.txt"
guard_fixture "$WORK/tiger_sentence.custom.yaml" "$FIXTURE/tiger_sentence.custom.yaml" \
    "方案补丁 tiger_sentence.custom.yaml（tab_learning: true）"
grep -q 'tiger_sentence/tab_learning: true' "$FIXTURE/tiger_sentence.custom.yaml" \
    || { echo "生成失败：夹具未开启 tab_learning: true（本金样失去意义）" >&2; exit 1; }

cp "$FIXTURE/symbols.yaml" "$user/symbols.yaml"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"
cp "$FIXTURE/tiger_sentence.custom.yaml" "$user/tiger_sentence.custom.yaml"

default_yaml "$shared/default.yaml"

plugin="$(require_lua_plugin)"
g++ -std=c++17 -O2 "$ROOT/tools/probes/rime_sequence_probe.cpp" -lrime -ldl -o "$WORK/probe"

lua_sha="$(git -C "$REF" show "$PIN:lua/tiger_sentence.lua" | sha256sum | cut -d' ' -f1)"
librime_version="$(pkg-config --modversion rime 2>/dev/null || true)"
{
    printf '# key_sequence_tab golden (Tab 锁路径；夹具 tab_learning: true)\n'
    printf '# reference: %s @ %s\n' "$REF_URL" "$PIN"
    printf '# tiger_sentence.lua sha256: %s\n' "$lua_sha"
    printf '# librime: %s; plugin: %s\n' "${librime_version:-unknown}" "$plugin"
    printf '# fixture: goldens/key_sequence_tab/tiger_sentence.custom.yaml (tab_learning: true)\n'
    LD_LIBRARY_PATH="$WORK${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$WORK/probe" "$user" "$shared" "$plugin" "$CASES"
} > "$WORK/golden.tsv"

# 写库前断言 + 原子写库：至少 1 个用例，且至少一步 `Tab` 被消费（夹具没生效就会失败）。
golden_require "$WORK/golden.tsv" '^case' "金样不含任何用例"
awk -F'\t' '$1=="step" && $4=="Tab" && $5=="1"{found=1} END{exit !found}' "$WORK/golden.tsv" \
    || { echo "生成失败：金样里没有任何被消费的 Tab 步（夹具 tab_learning 未生效？）" >&2; exit 1; }
write_golden "$WORK/golden.tsv" "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | grep -c '^step') steps)"
