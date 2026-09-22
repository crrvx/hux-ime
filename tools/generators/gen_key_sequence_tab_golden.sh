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

[ -f "$CASES" ] || { echo "缺少用例文件 $CASES" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tiger-keyseq-tab-XXXXXX")"
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

# ---- 夹具：入库内容为唯一来源（漂移即失败，不静默重写）--------------------------
mkdir -p "$FIXTURE"
git -C "$REF" show "$PIN:symbols.yaml" > "$WORK/symbols.yaml"
python3 - "$WORK" <<'PY'
import pathlib
import sys
work = pathlib.Path(sys.argv[1])
table = ["刘\tvp", "甲\tab", "乙\tab", "一\tcd", "第7\tz"]
table += [f"{chr(0x4E00 + i)}\tja" for i in range(22)]
(work / "tiger_sentence.codes.txt").write_text("\n".join(table) + "\n", encoding="utf-8")
(work / "tiger_sentence.custom.yaml").write_text(
    "patch:\n  tiger_sentence/high_freq_limit: 0\n"
    "  tiger_sentence/tab_learning: true\n", encoding="utf-8")
PY
for name in symbols.yaml tiger_sentence.codes.txt tiger_sentence.custom.yaml; do
    if [ ! -f "$FIXTURE/$name" ]; then
        echo "缺少入库夹具 $FIXTURE/$name（首次生成时请先放入再提交）" >&2
        exit 1
    fi
    if ! cmp -s "$WORK/$name" "$FIXTURE/$name"; then
        echo "夹具与 pin/补丁不符：$FIXTURE/$name" >&2
        echo "  —— 若确为上游 pin 变化，请人工确认后更新该文件；夹具内容不得由生成器静默改写。" >&2
        exit 1
    fi
done
grep -q 'tiger_sentence/tab_learning: true' "$FIXTURE/tiger_sentence.custom.yaml" \
    || { echo "夹具未开启 tab_learning: true（本金样失去意义）" >&2; exit 1; }

cp "$FIXTURE/symbols.yaml" "$user/symbols.yaml"
cp "$FIXTURE/tiger_sentence.codes.txt" "$user/tiger_sentence.codes.txt"
cp "$FIXTURE/tiger_sentence.custom.yaml" "$user/tiger_sentence.custom.yaml"

# 最小共享数据（与参照集成测试同构；页大小与重放侧 `DEFAULT_PAGE_SIZE` 一致）。
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
[ -f "$plugin" ] || { echo "缺少 librime-lua 插件：$plugin（可用 LUA_PLUGIN 覆盖）" >&2; exit 1; }
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
} | gzip -9 > "$OUT.tmp.$$"

# 写库前断言：至少 1 个用例，且至少一步 `Tab` 被消费（夹具没生效就会失败）。
if ! gzip -cd "$OUT.tmp.$$" | grep -q '^case'; then
    rm -f "$OUT.tmp.$$"
    echo "生成失败：$OUT 不含任何用例（检查 CASES 是否为空或全为注释）" >&2
    exit 1
fi
if ! gzip -cd "$OUT.tmp.$$" | awk -F'\t' '$1=="step" && $4=="Tab" && $5=="1"{found=1} END{exit !found}'; then
    rm -f "$OUT.tmp.$$"
    echo "生成失败：$OUT 里没有任何被消费的 Tab 步（夹具 tab_learning 未生效？）" >&2
    exit 1
fi
mv "$OUT.tmp.$$" "$OUT"

echo "wrote $OUT ($(gzip -cd "$OUT" | grep -c '^step') steps)"
