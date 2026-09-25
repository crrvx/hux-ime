#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 虎虚（hux-ime）一键安装：①依赖检查 ②构建 ③安装 ④校验 ⑤提示。
#   ./install.sh [-s|--system] [-u|--user] [--dry-run]
#   -s, --system  装到系统级 /usr（缺省；需要 sudo）
#   -u, --user    装到用户级 $HOME/.local，并写 environment.d 让 fcitx5 找到插件
#   --dry-run     只打印将执行的命令，不安装
# 两种级别互斥。装完不自动重启 fcitx5：按结尾提示自行重启。
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
cd "$root"

# ---------------------------------------------------------------- 公共（颜色 / 输出 / 执行）

# 颜色只在终端上给出：非 TTY（重定向、管道、CI 日志）或设了 NO_COLOR 时退化为纯文本。
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    c_orange=$'\033[38;5;208m' # 一般文字
    c_green=$'\033[32m'        # 网址 / 命令
    c_gray=$'\033[38;5;245m'   # 注释 / 次要说明
    c_white=$'\033[97m'        # 建议 / 声明 / 感谢
    c_cyan=$'\033[36m'         # 许可证名（GPL-3.0-or-later）
    c_reset=$'\033[0m'
else
    c_orange='' c_green='' c_gray='' c_white='' c_cyan='' c_reset=''
fi

info() { printf '%s%s%s\n' "$c_orange" "$*" "$c_reset"; } # 一般文字
green() { printf '%s%s%s\n' "$c_green" "$*" "$c_reset"; } # 网址 / 命令
note() { printf '%s%s%s\n' "$c_gray" "$*" "$c_reset"; }   # 注释 / 次要说明
white() { printf '%s%s%s\n' "$c_white" "$*" "$c_reset"; } # 建议 / 声明 / 感谢
step() { printf '\n%s%s%s\n' "$c_orange" "$*" "$c_reset"; }
die() {
    printf '%s%s%s\n' "$c_orange" "$*" "$c_reset" >&2
    exit 1
}

# 白色正文 + 绿色网址（建议 / 声明的同一行）。
white_url() {
    printf '%s%s%s%s%s%s\n' "$c_white" "$1" "$c_reset" "$c_green" "$2" "$c_reset"
}

# 白色正文 + 青色许可证名 + 白色续文（声明行内混色）。
white_license() {
    printf '%s%s%s%s%s%s%s\n' "$c_white" "$1" "$c_cyan" "$2" "$c_white" "$3" "$c_reset"
}

# 执行命令；--dry-run 只打印（绿色 `+` 前缀）。
run() {
    printf '%s+' "$c_green"
    printf ' %q' "$@"
    printf '%s\n' "$c_reset"
    if [ "$dry_run" -eq 0 ]; then
        "$@"
    fi
}

usage() {
    info "虎虚（hux-ime）一键安装"
    printf '\n'
    green "  ./install.sh [-s|--system] [-u|--user] [--dry-run]"
    printf '\n'
    printf '%s\n' '  -s, --system  装到系统级 /usr（缺省；需要 sudo）'
    printf '%s\n' '  -u, --user    装到用户级 $HOME/.local，并写 environment.d 让 fcitx5 找到插件'
    printf '%s\n' '  --dry-run     只打印将执行的命令，不安装'
    printf '%s\n' '  -h, --help    显示本帮助'
    printf '\n'
    printf '%s\n' '  -s 与 -u 互斥。构建在 build/addon；随包数据与主题按清单逐条核对落盘。'
    printf '%s\n' '  用户级安装需在环境里带上 FCITX_ADDON_DIRS（脚本会写 environment.d 并检测是否已继承）。'
}

# ---------------------------------------------------------------- 参数

mode='' # system | user（-s / -u 二选一）
dry_run=0

for arg in "$@"; do
    case "$arg" in
    -s | --system)
        if [ "$mode" = user ]; then die "-s 与 -u 互斥：只选一种安装级别"; fi
        mode=system
        ;;
    -u | --user)
        if [ "$mode" = system ]; then die "-s 与 -u 互斥：只选一种安装级别"; fi
        mode=user
        ;;
    --dry-run) dry_run=1 ;;
    -h | --help)
        usage
        exit 0
        ;;
    *) die "未知参数：$arg（用法：./install.sh [-s|--system] [-u|--user] [--dry-run]）" ;;
    esac
done
if [ -z "$mode" ]; then mode=system; fi

# 两种级别的落点：系统级沿用 fcitx5 自身的绝对 addon 目录；用户级把前缀指到 ~/.local，
# 并用相对 addon 目录（HUX_RELATIVE_ADDON_DIR）让插件库落在 ~/.local/lib/fcitx5。
if [ "$mode" = user ]; then
    if [ -z "${HOME:-}" ]; then die "用户级安装需要 HOME 环境变量"; fi
    prefix="$HOME/.local"
    addon_relative=ON
    mode_label="用户级 $prefix"
    env_file="${XDG_CONFIG_HOME:-$HOME/.config}/environment.d/90-hux.conf"
    env_line='FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5'
else
    prefix=/usr
    addon_relative=OFF
    mode_label="系统级 $prefix"
fi
build_dir=build/addon
data_root="$prefix/share/fcitx5/hux"
theme_root="$prefix/share/fcitx5/themes"

# ---------------------------------------------------------------- ① 依赖检查

check_dependencies() {
    if [ "$(id -u)" -eq 0 ]; then die "请以普通用户运行（系统级安装会在需要时调用 sudo）。"; fi
    local -a tools=(cmake cargo nproc)
    local -a missing=()
    local tool
    if [ "$mode" = system ]; then tools+=(sudo); fi
    for tool in "${tools[@]}"; do
        if ! command -v "$tool" >/dev/null 2>&1; then missing+=("$tool"); fi
    done
    if [ "${#missing[@]}" -gt 0 ]; then
        info "缺少依赖：${missing[*]}"
        note "请先安装（示例）："
        note "  Arch:          sudo pacman -S --needed cmake rust fcitx5 coreutils"
        note "  Fedora:        sudo dnf install cmake gcc-c++ rust fcitx5-devel coreutils"
        note "  Debian/Ubuntu: sudo apt install cmake g++ cargo libfcitx5core-dev coreutils"
        exit 1
    fi
    note "  已具备：${tools[*]}"
}

# ---------------------------------------------------------------- ② 构建

build_addon() {
    # 安装前缀与 addon 目录形态每次都显式给出：两种级别共用 build/addon，CMake 缓存随之更新。
    run cmake -S platform/fcitx5 -B "$build_dir" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$prefix" \
        -DHUX_RELATIVE_ADDON_DIR="$addon_relative"
    run cmake --build "$build_dir" -j "$(nproc)"
    note "  构建目录 $build_dir（cargo 首次编译较慢）"
}

# ---------------------------------------------------------------- ③ 安装

install_files() {
    if [ "$mode" = system ]; then
        run sudo cmake --install "$build_dir"
    else
        run cmake --install "$build_dir"
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        # hicolor 未刷新时部分桌面看不到新图标；缺工具就跳过，不算失败。
        if [ "$mode" = system ]; then
            run sudo gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" || true
        else
            run gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" || true
        fi
    fi
    if [ "$mode" = user ]; then
        write_environment_d
        report_addon_dir_env
    fi
}

# 用户级 addon 目录不在 fcitx5 的缺省搜索集里：写 environment.d，让 systemd 用户实例带
# FCITX_ADDON_DIRS 启动 fcitx5。该变量**取代**缺省值，故显式带上系统目录；
# systemd 在登录时读这些文件并展开 $HOME（见 environment.d(5)）。已存在且内容相同则不改写。
write_environment_d() {
    local content="# 虎虚（hux-ime）用户级插件目录（install.sh -u 写入）
$env_line"
    if [ "$dry_run" -eq 1 ]; then
        note "  （dry-run）将写入 $env_file："
        note "    $env_line"
        return 0
    fi
    run mkdir -p "$(dirname "$env_file")"
    if [ ! -e "$env_file" ]; then
        printf '%s\n' "$content" >"$env_file"
        note "  已写入 $env_file：$env_line"
    elif [ "$(cat "$env_file")" = "$content" ]; then
        note "  已存在且内容相同，未改写 $env_file"
    else
        printf '%s\n' "$content" >"$env_file"
        note "  内容不同，已改写 $env_file：$env_line"
    fi
}

# environment.d 的变量由 systemd 用户实例在**登录时**读入：本次会话可能还没有它。
# 这里检测当前会话是否已继承，未继承就明说（不假装成功）。
report_addon_dir_env() {
    if [ "$dry_run" -eq 1 ]; then
        note "  （dry-run）将检测当前会话是否已继承 FCITX_ADDON_DIRS"
        return 0
    fi
    if systemctl --user show-environment 2>/dev/null | grep -q '^FCITX_ADDON_DIRS='; then
        info "  当前会话已继承 FCITX_ADDON_DIRS：重启 fcitx5 即可加载插件。"
        return 0
    fi
    info "  当前会话不会继承该变量：请在启动 fcitx5 前 export 它，或改用 ./install.sh -s。"
    green "    export $env_line"
    note "    （environment.d 的变量在下次登录后由 systemd 用户实例带入；没有 systemd 用户实例时只能 export。）"
}

# ---------------------------------------------------------------- ④ 校验落盘

# 两份清单（data/MANIFEST、assets/themes/MANIFEST）逐条核对：装出的布局必须与清单一致，
# 缺任一即失败——只走 CMake 安装时「无词库引擎」的缺口在此暴露。
verify_files() {
    local count=0 entry dest
    if [ ! -f data/MANIFEST ]; then die "缺少 data/MANIFEST（随包数据清单）"; fi
    if [ ! -f assets/themes/MANIFEST ]; then die "缺少 assets/themes/MANIFEST（共享主题清单）"; fi
    while IFS= read -r entry; do
        case "$entry" in '' | '#'*) continue ;; esac
        dest="$data_root/$(basename "$entry")"
        count=$((count + 1))
        if [ "$dry_run" -eq 1 ]; then
            note "  （dry-run）应有 $dest"
            continue
        fi
        if [ ! -f "$dest" ]; then
            die "缺少随包数据 $dest（CMake 安装规则应与 data/MANIFEST 一致：platform/fcitx5/CMakeLists.txt；自检 bash tools/checks/check_data_manifest.sh）"
        fi
    done <data/MANIFEST
    if [ "$count" -eq 0 ]; then die "data/MANIFEST 没有有效行（随包数据清单缺失或为空）"; fi
    note "  随包数据 $count 项 → $data_root/"
    count=0
    while IFS= read -r entry; do
        case "$entry" in '' | '#'*) continue ;; esac
        dest="$theme_root/$entry/theme.conf"
        count=$((count + 1))
        if [ "$dry_run" -eq 1 ]; then
            note "  （dry-run）应有 $dest"
            continue
        fi
        if [ ! -f "$dest" ]; then
            die "缺少主题 $dest（CMake 安装规则应与 assets/themes/MANIFEST 一致：platform/fcitx5/CMakeLists.txt）"
        fi
    done <assets/themes/MANIFEST
    if [ "$count" -eq 0 ]; then die "assets/themes/MANIFEST 没有有效行（共享主题清单缺失或为空）"; fi
    note "  共享主题 $count 套 → $theme_root/"
}

# ---------------------------------------------------------------- ⑤ 装完提示

print_hints() {
    printf '\n'
    info "安装完成：$mode_label"
    printf '\n'
    info "  1. 请重启 fcitx5："
    green "       nohup fcitx5 -r -d >/dev/null 2>&1 &"
    info "  2. 在 fcitx5-configtool 中选择「虎虚 / hux」"
    info "  3. （可选）模型 sentence-ngram-mobile.bin 需单独获取："
    green "       QQ 群 948170058"
    green "       https://github.com/lvyww/tiger-sentense-rime/releases"
    if [ "$mode" = system ]; then
        info "  4. （可选）把模型放到系统级目录："
        green "       sudo mkdir -p /usr/share/fcitx5/hux/models/ && sudo mv -i <模型下载后的当前位置> /usr/share/fcitx5/hux/models/"
    else
        info "  4. （可选）把模型放到用户级目录："
        green "       mkdir -p ~/.local/share/fcitx5/hux/models/ && mv -i <模型下载后的当前位置> ~/.local/share/fcitx5/hux/models/"
    fi
    printf '\n'
    note "  图标：托盘显示「虍」是经典界面开了「优先使用文字图标」（configtool 可关）；"
    note "       重装后仍是旧图标时，重启 fcitx5 与桌面面板（KDE：kquitapp6 plasmashell && kstart plasmashell）。"
    printf '\n'
    white_url "  建议：如有任何改进建议，欢迎在此留痕：" "https://github.com/crrvx/hux-ime/issues"
    white_license "  声明：虎虚（hux-ime）以 " "GPL-3.0-or-later" " 开源，© 2026 明雅流风。"
    white_url "  项目地址：" "https://github.com/crrvx/hux-ime"
    white "  感谢使用与收藏。"
}

# ---------------------------------------------------------------- 主流程

step "①依赖检查"
check_dependencies
step "②构建"
build_addon
step "③安装（$mode_label）"
install_files
step "④校验落盘"
verify_files
step "⑤提示"
print_hints

if [ "$dry_run" -eq 1 ]; then
    printf '\n'
    note "（--dry-run：以上命令均未实际执行）"
fi
