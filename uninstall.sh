#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 虎虚（hux-ime）一键卸载：移除系统级插件与数据。
#   ./uninstall.sh [--purge] [--dry-run]
#   --purge    同时清除用户数据（选项、学习库、模型）
#   --dry-run  只打印将执行的命令
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
cd "$root"

purge=0
dry_run=0
for arg in "$@"; do
    case "$arg" in
        --purge) purge=1 ;;
        --dry-run) dry_run=1 ;;
        -h|--help)
            sed -n '5,8p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "未知参数：$arg（用法：./uninstall.sh [--purge] [--dry-run]）" >&2
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

if ! command -v sudo >/dev/null 2>&1; then
    echo "缺少依赖：sudo" >&2
    exit 1
fi

echo "[1/2] 移除系统级文件（需要 sudo）……"
run sudo rm -f \
    /usr/lib/fcitx5/libhux.so \
    /usr/share/fcitx5/addon/hux.conf \
    /usr/share/fcitx5/inputmethod/hux.conf
run sudo rm -rf /usr/share/fcitx5/hux

if [ "$purge" -eq 1 ]; then
    echo "[2/2] 清除用户数据（选项 / 学习库 / 模型）……"
    run rm -rf \
        "$HOME/.local/share/fcitx5/hux" \
        "$HOME/.config/fcitx5/conf/hux.conf"
else
    echo "[2/2] 保留用户数据（选项 / 学习库 / 模型）："
    echo "   $HOME/.local/share/fcitx5/hux/"
    echo "   $HOME/.config/fcitx5/conf/hux.conf"
    echo "   （如需彻底清除：./uninstall.sh --purge）"
fi

echo "重启 fcitx5……"
if pgrep -x fcitx5 >/dev/null 2>&1; then
    run fcitx5 -r -d
else
    echo "（未检测到运行中的 fcitx5，跳过）"
fi

echo "卸载完成。"

if [ "$dry_run" -eq 1 ]; then
    echo
    echo "（--dry-run：以上命令均未实际执行）"
fi
