#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 生成 TCSKNM03 五阶模型 fixture（`goldens/fivegram_fixture.bin`）。
#
# 上游 `tools/model_fixture.lua` 只写 TCSKNM02，五阶格式必须用上游 builder：
#   build_tcs_knm03 <arpa> <model-out> <work-dir>
# ARPA 取自同一 pin 的 `tools/test_tcs_knm03.py`（`ARPA = r"""…"""`），故 fixture 由 pin 完全决定、可复现。
#
# 用法：tools/generators/gen_fivegram_fixture.sh [输出文件]
#   REF  参照仓库本地检出（默认 _external/tiger-sentense-rime）
#   PIN  参照固定提交（默认主干 pin）
#
# 依赖：git、g++、python3。金样不在 CI 重生成（依赖具体 g++/上游源码），需本地手动执行。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF="${REF:-${HUX_REFERENCE_REPO:-$ROOT/_external/tiger-sentense-rime}}"
[ -d "$REF" ] || { echo "参照检出不存在：$REF（用 REF=… 指定）" >&2; exit 2; }
PIN="${PIN:-9f742d275c2bd50c7c664be1c258a7b8429e83a1}"
OUT="${1:-$ROOT/goldens/fivegram_fixture.bin}"
EXPECT_SHA="0c3581b2b84baa7e782de25bcbe513e26fabb8835e9b2f370de88a614f8ebda9"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/hux-fivegram-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

git -C "$REF" show "$PIN:tools/build_tcs_knm03.cpp" > "$WORK/build.cpp"
git -C "$REF" show "$PIN:tools/test_tcs_knm03.py" > "$WORK/test.py"

python3 - "$WORK/test.py" "$WORK/fixture.arpa" <<'PY'
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
matched = re.search(r'ARPA = r"""(.*?)"""', source, re.S)
if not matched:
    raise SystemExit("未在 tools/test_tcs_knm03.py 中找到 ARPA")
pathlib.Path(sys.argv[2]).write_text(matched.group(1), encoding="utf-8")
PY

g++ -std=c++20 -O2 "$WORK/build.cpp" -o "$WORK/build"
mkdir -p "$WORK/work"
"$WORK/build" "$WORK/fixture.arpa" "$OUT" "$WORK/work" > /dev/null

sha="$(sha256sum "$OUT" | cut -d' ' -f1)"
[ "$sha" = "$EXPECT_SHA" ] || {
    echo "fixture sha256 不符：$sha（期望 $EXPECT_SHA）" >&2
    exit 1
}
echo "生成 $OUT（$(stat -c%s "$OUT") 字节，sha256 $sha）"
