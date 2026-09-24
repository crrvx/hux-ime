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

# 用户可写目录按与引擎相同的 XDG 回退解析（引擎优先 XDG_DATA_HOME / XDG_CONFIG_HOME）。
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"

echo "[1/2] 移除系统级文件（需要 sudo）……"
# 插件库目录：兼容 lib 与 lib64（Fedora 等）。
for libdir in /usr/lib/fcitx5 /usr/lib64/fcitx5; do
    [ -f "$libdir/libhux.so" ] && run sudo rm -f "$libdir/libhux.so"
done
run sudo rm -f \
    /usr/share/fcitx5/addon/hux.conf \
    /usr/share/fcitx5/inputmethod/hux.conf \
    /usr/share/icons/hicolor/scalable/apps/hux.svg \
    /usr/share/icons/hicolor/48x48/apps/hux.png \
    /usr/share/icons/hicolor/22x22/apps/hux.png
if [ "$purge" -eq 1 ]; then
    # 连同系统级数据目录（含用户自取的 models/）一并删除。
    if [ -d /usr/share/fcitx5/hux/models ]; then
        echo "  提示：/usr/share/fcitx5/hux/models/ 下的模型（不随包）将一并删除。"
    fi
    run sudo rm -rf /usr/share/fcitx5/hux
else
    # 只删随包数据文件，保留用户自取的 models/（README 推荐放在这里）。
    # 删除清单与 install.sh / CMake 同源：`data/MANIFEST`（
    # 此前这里枚举 7 个文件名、而安装侧用 glob，data/ 增删文件就会残留）。
    data_files=()
    while IFS= read -r entry; do
        case "$entry" in ''|'#'*) continue ;; esac
        data_files+=("/usr/share/fcitx5/hux/$(basename "$entry")")
    done < data/MANIFEST
    if [ "${#data_files[@]}" -eq 0 ]; then
        echo "data/MANIFEST 没有有效行（随包数据清单缺失或为空）" >&2
        exit 1
    fi
    run sudo rm -f "${data_files[@]}"
    echo "  （模型与其它自建文件保留在 /usr/share/fcitx5/hux/；彻底清除用 --purge）"
fi

# 共享主题（fcitx5 主题形态）：清单与 CMake / install.sh 同源。
if [ -f assets/themes/MANIFEST ]; then
    theme_dirs=()
    while IFS= read -r entry; do
        case "$entry" in ''|'#'*) continue ;; esac
        theme_dirs+=("/usr/share/fcitx5/themes/$entry")
    done < assets/themes/MANIFEST
    if [ "${#theme_dirs[@]}" -gt 0 ]; then
        run sudo rm -rf "${theme_dirs[@]}"
        # 仅当目录已空时收掉它——非空说明还有别人装的主题，别动。
        run sudo rmdir --ignore-fail-on-non-empty /usr/share/fcitx5/themes 2>/dev/null || true
        echo "  已移除主题 ${#theme_dirs[@]} 套（/usr/share/fcitx5/themes/）"
    fi
else
    echo "缺少 assets/themes/MANIFEST，跳过主题清理" >&2
fi

if [ "$purge" -eq 1 ]; then
    echo "[2/2] 清除用户数据（选项 / 学习库 / 模型）……"
    run rm -rf "$data_home/fcitx5/hux" "$config_home/fcitx5/conf/hux.conf"
else
    echo "[2/2] 保留用户数据（选项 / 学习库 / 模型）："
    echo "   $data_home/fcitx5/hux/"
    echo "   $config_home/fcitx5/conf/hux.conf"
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
