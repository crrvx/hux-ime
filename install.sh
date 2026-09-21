#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 虎虚（hux-ime）一键安装：构建 addon → 安装插件与随包数据 → 重启提示。
#   ./install.sh [--dry-run]     # --dry-run 只打印将执行的命令
# n-gram 模型不随包，安装完成后按结尾提示自行获取。
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
cd "$root"

dry_run=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry_run=1 ;;
        -h|--help)
            sed -n '5,7p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "未知参数：$arg（用法：./install.sh [--dry-run]）" >&2
            exit 1
            ;;
    esac
done

run() {
    if [ "$dry_run" -eq 1 ]; then
        printf '+'
        printf ' %q' "$@"
        printf '\n'
    else
        "$@"
    fi
}

if [ "$(id -u)" -eq 0 ]; then
    echo "请以普通用户运行（脚本会在需要时调用 sudo）。" >&2
    exit 1
fi

for tool in cmake cargo sudo install nproc pgrep; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "缺少依赖：$tool" >&2
        cat >&2 <<'EOF'
请先安装依赖（示例）：
  Arch:          sudo pacman -S --needed cmake rust fcitx5 coreutils procps-ng
  Fedora:        sudo dnf install cmake gcc-c++ rust fcitx5-devel coreutils procps-ng
  Debian/Ubuntu: sudo apt install cmake g++ cargo libfcitx5core-dev coreutils procps
EOF
        exit 1
    fi
done

echo "[1/4] 构建 addon（cargo + cmake，首次较慢）……"
run cmake -S platform/fcitx5 -B build/addon \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
run cmake --build build/addon -j "$(nproc)"

echo "[2/4] 安装插件到 /usr/lib/fcitx5（需要 sudo）……"
run sudo cmake --install build/addon

echo "[3/4] 安装随包数据到 /usr/share/fcitx5/hux/……"
run sudo install -d /usr/share/fcitx5/hux
run sudo install -m644 data/tiger_sentence.* data/symbols.yaml \
    /usr/share/fcitx5/hux/

echo "[4/4] 重启 fcitx5……"
if pgrep -x fcitx5 >/dev/null 2>&1; then
    run fcitx5 -r -d
else
    echo "（未检测到运行中的 fcitx5，跳过）"
fi

cat <<'EOF'

安装完成。接下来：
  1. 在 fcitx5 配置工具里「添加输入法」→ 虎虚（hux）。
  2. （可选）n-gram 模型不随包，但能明显提升整句质量：
     下载：https://github.com/lvyww/tiger-sentense-rime/releases/tag/model
     放置：~/.local/share/fcitx5/hux/models/sentence-ngram-mobile.bin
     或系统级：/usr/share/fcitx5/hux/models/（与 rime 共用同一份也可）

卸载：./uninstall.sh（保留用户数据）；./uninstall.sh --purge（连用户数据一起清除）。
EOF

if [ "$dry_run" -eq 1 ]; then
    echo
    echo "（--dry-run：以上命令均未实际执行）"
fi
