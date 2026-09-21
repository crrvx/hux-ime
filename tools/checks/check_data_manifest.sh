#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 「装 / 卸 / CMake」三处读同一份随包数据清单，且清单与 data/ 实况一致（复核整改第 4 批 M15/F5）。
#
#   bash tools/checks/check_data_manifest.sh
#
# 检查项（任一不符即 exit 1）：
#   ① data/MANIFEST 存在、至少 1 条有效行、无重复、每行都在 data/ 下且文件存在；
#   ② `data/` 里 `tiger_sentence.*` 与 `symbols.yaml`（= 旧 install.sh glob 的覆盖范围）全部在清单里；
#   ③ install.sh 与 uninstall.sh 都引用 data/MANIFEST（不再各自维护名单）；
#   ④ platform/fcitx5/CMakeLists.txt 引用 data/MANIFEST，且安装目标为 share/fcitx5/hux。
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
manifest=data/MANIFEST
failed=0

fail() {
    echo "FAIL $*" >&2
    failed=1
}

if [ ! -f "$manifest" ]; then
    echo "FAIL 缺少 $manifest" >&2
    exit 1
fi

# ① 清单自身
entries=$(sed -e 's/[[:space:]]*$//' "$manifest" | grep -vE '^[[:space:]]*(#|$)' || true)
count=$(printf '%s\n' "$entries" | grep -c . || true)
if [ "$count" -lt 1 ]; then
    fail "$manifest 没有有效行"
fi
duplicates=$(printf '%s\n' "$entries" | sort | uniq -d || true)
if [ -n "$duplicates" ]; then
    fail "$manifest 有重复行：$duplicates"
fi
while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    case "$entry" in
        data/*) ;;
        *) fail "清单行不在 data/ 下：$entry" ;;
    esac
    if [ ! -f "$entry" ]; then
        fail "清单列出的文件不存在：$entry"
    fi
done <<<"$entries"

# ② glob 时代的覆盖范围必须全在清单里（将来 data/ 增删文件时自动对账）
for path in data/tiger_sentence.* data/symbols.yaml; do
    [ -e "$path" ] || continue
    if ! printf '%s\n' "$entries" | grep -qxF "$path"; then
        fail "$path 存在但未登记在 $manifest（会被 install.sh 时代装上却无人卸载）"
    fi
done

# ③ 两个脚本都读清单
for script in install.sh uninstall.sh; do
    if ! grep -q 'data/MANIFEST' "$script"; then
        fail "$script 未引用 data/MANIFEST（装/卸清单会各自漂移）"
    fi
done

# ④ CMake 安装规则读清单且装到引擎查找的数据目录
# 注意锚定行首：注释掉的规则（`# install(FILES …)`）不得算通过。
cmake=platform/fcitx5/CMakeLists.txt
if ! grep -qE '^[[:space:]]*file\(STRINGS[^#]*data/MANIFEST' "$cmake"; then
    fail "$cmake 未从 data/MANIFEST 读取文件清单（只走 cmake --install 会得到无词库引擎）"
fi
if ! grep -qE '^[[:space:]]*install\(FILES[^#]*HUX_DATA_FILES' "$cmake"; then
    fail "$cmake 未把 data/MANIFEST 读出的文件加入 install(FILES …)"
fi
if ! grep -qE '^[[:space:]]*install\(FILES[^#]*DESTINATION[[:space:]]+share/fcitx5/hux' "$cmake"; then
    fail "$cmake 未把清单文件装到 share/fcitx5/hux"
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi
echo "check_data_manifest: $count 条随包数据，装/卸/CMake 三处清单一致"
