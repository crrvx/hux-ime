#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# live-probe.sh —— 在**当前桌面会话**里诊断「字反查」的 AT-SPI 取字来源（一键跑）。
#
#   bash tools/atspi/live-probe.sh [轮数]     默认 100 轮 × 200 ms = 20 s
#
# 它连的是你这个会话的无障碍总线（**不是** run.sh 那个私有总线），用来回答：
#   · 无障碍总线通不通（不通 ⇒ 无障碍没开，虎虚只能退回「最近上屏文本」）；
#   · 浏览器/终端里的输入框有没有 FOCUSED 节点、有没有 Text 接口、CaretOffset 读不读得到；
#   · 移动光标时，无障碍侧的文本与光标跟不跟得上（这是与插件行为对照的那一半）。
#
# 退出码：0 = 正常跑完；2 = 环境缺依赖（atspi-2 开发包）或无障碍总线不可用。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT}/build/atspi-live"
ROUNDS="${1:-100}"

if ! command -v g++ >/dev/null 2>&1; then
    echo "缺少 g++（装 gcc / base-devel）。" >&2
    exit 2
fi
if ! pkg-config --exists atspi-2; then
    echo "缺少 atspi-2 开发包：装 libatspi2.0-dev（Arch: at-spi2-core）。" >&2
    echo "顺带看一眼插件构建时有没有带上 AT-SPI：" >&2
    echo "  ldd /usr/lib/fcitx5/libhux.so | grep -i atspi" >&2
    exit 2
fi
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" && -z "${AT_SPI_BUS_ADDRESS:-}" ]]; then
    echo "看不到会话总线（DBUS_SESSION_BUS_ADDRESS 为空）：" >&2
    echo "请在桌面会话的终端里跑，别在 ssh / 私有总线里跑。" >&2
    exit 2
fi

mkdir -p "${OUT_DIR}"
echo "编译诊断探针 → ${OUT_DIR}/live_probe"
g++ -O1 -g -std=c++17 -Wall -Wextra \
    "${ROOT}/tools/atspi/live_probe.cpp" \
    -o "${OUT_DIR}/live_probe" \
    $(pkg-config --cflags --libs atspi-2 gobject-2.0)

echo "== 请在目标应用的输入框里点一下，再用 ←/→ 移动光标 =="
exec "${OUT_DIR}/live_probe" "${ROUNDS}"
