#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 虎虚（hux-ime）卸载：①探测安装 ②确认可选项 ③移除 ④结果清单。
#   ./uninstall.sh [--dry-run]
#   --dry-run  只打印将执行的命令与「计划删除清单」：不提问、不上色、不删除
#   -h, --help 显示本帮助
# 全交互：先探测系统级与用户级两处安装，只对存在的项提问与操作；主题缺省卸载，
# 模型（体积大、可复用）与用户数据缺省保留。卸载结束不自动重启 fcitx5。
#
# `--dry-run` 的「计划删除清单」契约（机器可解析；守卫 tools/checks/check_uninstall_clean.py 按它解析）：
#   每行 `<标记> <绝对路径>`，标记两种：`-` = 按缺省会删；`?` = 需交互确认（回答 y）后才删，
#   即模型 / 用户数据这类缺省保留的项。
#   路径为绝对路径，一行一条；整个目录以 `/` 结尾；通配用 `*`（如 multiarch 的 addon 目录）。
#   清单按固定落点与两份清单静态给出、不按探测结果过滤：干净机器上也能取到完整的可卸载集合。
#   其余人读信息照常打印，守卫只认上述两种前缀行。
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
cd "$root"

# ---------------------------------------------------------------- 公共（颜色 / 输出 / 执行）

# 颜色只在终端上给出：非 TTY（重定向、管道、CI 日志）、设了 NO_COLOR，或 `--dry-run`
# 时退化为纯文本——预览的「计划删除清单」要机器可解析（契约见文件头）。
color=1
if [ ! -t 1 ] || [ -n "${NO_COLOR:-}" ]; then color=0; fi
for arg in "$@"; do
    if [ "$arg" = --dry-run ]; then color=0; fi
done
if [ "$color" -eq 1 ]; then
    c_orange=$'\033[38;5;208m' # 一般文字
    c_green=$'\033[32m'        # 网址 / 命令
    c_gray=$'\033[38;5;245m'   # 注释 / 次要说明
    c_white=$'\033[97m'        # 建议 / 感谢
    c_reset=$'\033[0m'
else
    c_orange='' c_green='' c_gray='' c_white='' c_reset=''
fi

info() { printf '%s%s%s\n' "$c_orange" "$*" "$c_reset"; } # 一般文字
green() { printf '%s%s%s\n' "$c_green" "$*" "$c_reset"; } # 网址 / 命令
note() { printf '%s%s%s\n' "$c_gray" "$*" "$c_reset"; }   # 注释 / 次要说明
white() { printf '%s%s%s\n' "$c_white" "$*" "$c_reset"; } # 建议 / 感谢
step() { printf '\n%s%s%s\n' "$c_orange" "$*" "$c_reset"; }
die() {
    printf '%s%s%s\n' "$c_orange" "$*" "$c_reset" >&2
    exit 1
}

# 建议：白色正文 + 绿色网址（同一行）。
white_url() {
    printf '%s%s%s%s%s%s\n' "$c_white" "$1" "$c_reset" "$c_green" "$2" "$c_reset"
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

# 询问（缺省答案标在 [Y/n] / [y/N] 里；标准输入读不到时按缺省处理）。
ask() {
    local prompt=$1 default=$2 hint='y/N' answer=''
    if [ "$default" = y ]; then hint='Y/n'; fi
    printf '%s%s [%s] %s' "$c_orange" "$prompt" "$hint" "$c_reset"
    if ! read -r answer; then
        printf '\n'
        note "  （标准输入不可读，按缺省处理）"
    fi
    case "$answer" in
    y | Y | yes | YES | Yes) return 0 ;;
    n | N | no | NO | No) return 1 ;;
    '') [ "$default" = y ] ;;
    *)
        note "  （无法识别，按否处理）"
        return 1
        ;;
    esac
}

usage() {
    info "虎虚（hux-ime）卸载"
    printf '\n'
    green "  ./uninstall.sh [--dry-run]"
    printf '\n'
    printf '%s\n' '  --dry-run  只打印将执行的命令与「计划删除清单」：不提问、不上色、不删除'
    printf '%s\n' '  -h, --help 显示本帮助'
    printf '\n'
    printf '%s\n' '  缺省全交互，开始前依次询问（只问存在的项）：'
    printf '%s\n' '    是否卸载共享主题？[Y/n]   是否卸载模型？[y/N]   是否删除用户数据？[y/N]'
    printf '%s\n' '  模型体积大、可复用；用户数据指选项 / 学习库 / conf/hux.conf。其余（插件库、'
    printf '%s\n' '  {addon,inputmethod}/hux.conf、图标、随包数据、-u 写的 environment.d）缺省都卸。'
    printf '%s\n' '  要连模型与用户数据一起清除，就在对应的两问回答 y。'
    printf '\n'
    printf '%s\n' '  --dry-run 的「计划删除清单」契约：每行 `<标记> <绝对路径>`，标记 - 表示按缺省'
    printf '%s\n' '  会删、? 表示回答 y 才删（模型 / 用户数据）；绝对路径一行一条，整个目录以 / 结尾，'
    printf '%s\n' '  通配用 *（如 multiarch 的 addon 目录）。'
}

# ---------------------------------------------------------------- 参数

dry_run=0

for arg in "$@"; do
    case "$arg" in
    --dry-run) dry_run=1 ;;
    -h | --help)
        usage
        exit 0
        ;;
    *) die "未知参数：$arg（用法：./uninstall.sh [--dry-run]）" ;;
    esac
done

if [ -z "${HOME:-}" ]; then die "需要 HOME 环境变量"; fi

# ---------------------------------------------------------------- 落点（与 install.sh / CMake 同源）

user_prefix="$HOME/.local"                              # install.sh -u 的前缀：插件 / 随包数据 / 图标
user_share="$user_prefix/share"
user_env_file="${XDG_CONFIG_HOME:-$HOME/.config}/environment.d/90-hux.conf"

# 引擎自己的用户数据目录（选项 / 学习库）：按 XDG 解析，缺省与 $user_share/fcitx5/hux 相同。
engine_data_home="${XDG_DATA_HOME:-$user_share}"
engine_hux_dir="$engine_data_home/fcitx5/hux"
engine_conf="${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5/conf/hux.conf"

# 系统级插件库：兼容 lib、lib64 与 multiarch（Debian/Ubuntu：lib/<triplet>/fcitx5）。
sys_lib_patterns=(/usr/lib/fcitx5/libhux.so /usr/lib64/fcitx5/libhux.so '/usr/lib/*/fcitx5/libhux.so')
sys_conf_files=(/usr/share/fcitx5/addon/hux.conf /usr/share/fcitx5/inputmethod/hux.conf)
sys_icon_files=(
    /usr/share/icons/hicolor/scalable/apps/hux.svg
    /usr/share/icons/hicolor/48x48/apps/hux.png
    /usr/share/icons/hicolor/22x22/apps/hux.png
)
sys_themes_dir=/usr/share/fcitx5/themes
sys_hux_dir=/usr/share/fcitx5/hux
sys_models_dir="$sys_hux_dir/models"

user_lib="$user_prefix/lib/fcitx5/libhux.so"
user_conf_files=("$user_share/fcitx5/addon/hux.conf" "$user_share/fcitx5/inputmethod/hux.conf")
user_icon_files=(
    "$user_share/icons/hicolor/scalable/apps/hux.svg"
    "$user_share/icons/hicolor/48x48/apps/hux.png"
    "$user_share/icons/hicolor/22x22/apps/hux.png"
)
user_themes_dir="$user_share/fcitx5/themes"
user_hux_dir="$user_share/fcitx5/hux"
user_models_dir="$user_hux_dir/models"

# 随包数据与共享主题的名单来自两份清单（与 CMake / install.sh 同源）。
manifest_lines() {
    local line
    while IFS= read -r line; do
        case "$line" in '' | '#'*) continue ;; esac
        printf '%s\n' "$line"
    done <"$1"
}

if [ ! -f data/MANIFEST ]; then die "缺少 data/MANIFEST（随包数据清单）"; fi
if [ ! -f assets/themes/MANIFEST ]; then die "缺少 assets/themes/MANIFEST（共享主题清单）"; fi
data_names=()
while IFS= read -r entry; do data_names+=("$(basename "$entry")"); done < <(manifest_lines data/MANIFEST)
if [ "${#data_names[@]}" -eq 0 ]; then die "data/MANIFEST 没有有效行（随包数据清单缺失或为空）"; fi
theme_names=()
while IFS= read -r entry; do theme_names+=("$entry"); done < <(manifest_lines assets/themes/MANIFEST)
if [ "${#theme_names[@]}" -eq 0 ]; then die "assets/themes/MANIFEST 没有有效行（共享主题清单缺失或为空）"; fi

sys_data=()
user_data=()
sys_themes=()
user_themes=()
for name in "${data_names[@]}"; do
    sys_data+=("$sys_hux_dir/$name")
    user_data+=("$user_hux_dir/$name")
done
for name in "${theme_names[@]}"; do
    sys_themes+=("$sys_themes_dir/$name")
    user_themes+=("$user_themes_dir/$name")
done

# ---------------------------------------------------------------- 探测 / 删除

# 通配展开为「实际存在的路径」；无匹配时输出为空（不保留字面模式）。
expand_existing() {
    local pattern
    for pattern in "$@"; do
        compgen -G "$pattern" || true
    done
}

# 同一文件被多条模式命中去重（`/usr/lib64` 常是指向 `/usr/lib` 的符号链接）。
dedupe_paths() {
    local p real
    local -A seen=()
    while IFS= read -r p; do
        if [ -z "$p" ]; then continue; fi
        real=$(readlink -f -- "$p" 2>/dev/null) || real=$p
        if [ -z "${seen[$real]:-}" ]; then
            seen[$real]=1
            printf '%s\n' "$p"
        fi
    done
}

# 展示用：$HOME 前缀缩写成 ~，多条路径用「、」连接。
show_paths() {
    local p out=''
    for p in "$@"; do
        p="${p/#$HOME/~}"
        if [ -z "$out" ]; then out="$p"; else out="$out、$p"; fi
    done
    printf '%s' "$out"
}

# 批量删除：路径必须落在本次卸载涉及的目录前缀内（防数组意外为空或错拼时误删）。
remove_abs() {
    if [ "$#" -eq 0 ]; then return 0; fi
    local p prefix ok
    for p in "$@"; do
        ok=0
        for prefix in /usr "$user_prefix" "$engine_data_home" "${XDG_CONFIG_HOME:-$HOME/.config}"; do
            case "$p" in "$prefix"/*) ok=1 ;; esac
        done
        if [ "$ok" -eq 0 ]; then die "拒绝删除预期之外的路径：$p"; fi
    done
    if [ "$need_sudo" -eq 1 ]; then
        run sudo rm -rf -- "$@"
    else
        run rm -rf -- "$@"
    fi
}

# 收尾：目录空了就收掉（非空说明还有别的东西，留着）。
cleanup_empty_dirs() {
    local dir
    for dir in "$@"; do
        if [ -d "$dir" ] && [ -z "$(ls -A "$dir" 2>/dev/null)" ]; then
            if [ "$need_sudo" -eq 1 ]; then
                run sudo rmdir -- "$dir" || true
            else
                run rmdir -- "$dir" || true
            fi
        fi
    done
}

# 探测存在的项（交互问答、结果清单与真正的删除都基于它；--dry-run 的计划清单是静态的）。
probe() {
    mapfile -t sys_lib_present < <(expand_existing "${sys_lib_patterns[@]}" | dedupe_paths)
    mapfile -t sys_conf_present < <(expand_existing "${sys_conf_files[@]}" | dedupe_paths)
    mapfile -t sys_icon_present < <(expand_existing "${sys_icon_files[@]}" | dedupe_paths)
    mapfile -t sys_data_present < <(expand_existing "${sys_data[@]}" | dedupe_paths)
    mapfile -t sys_theme_present < <(expand_existing "${sys_themes[@]}" | dedupe_paths)
    mapfile -t sys_model_present < <(expand_existing "$sys_models_dir/*.bin" | dedupe_paths)
    mapfile -t user_lib_present < <(expand_existing "$user_lib" | dedupe_paths)
    mapfile -t user_conf_present < <(expand_existing "${user_conf_files[@]}" | dedupe_paths)
    mapfile -t user_icon_present < <(expand_existing "${user_icon_files[@]}" | dedupe_paths)
    mapfile -t user_data_present < <(expand_existing "${user_data[@]}" | dedupe_paths)
    mapfile -t user_theme_present < <(expand_existing "${user_themes[@]}" | dedupe_paths)
    mapfile -t user_model_present < <(expand_existing "$user_models_dir/*.bin" | dedupe_paths)
    mapfile -t user_env_present < <(expand_existing "$user_env_file" | dedupe_paths)
    mapfile -t user_option_present < <(expand_existing "$engine_hux_dir/tiger_sentence.options.yaml" "$engine_hux_dir/user.yaml" | dedupe_paths)
    mapfile -t user_learning_present < <(expand_existing "$engine_hux_dir"/tiger_sentence_learning_*.userdb | dedupe_paths)
    mapfile -t user_conf_file_present < <(expand_existing "$engine_conf" | dedupe_paths)
}

sys_found() {
    [ "${#sys_lib_present[@]}" -gt 0 ] || [ "${#sys_conf_present[@]}" -gt 0 ] ||
        [ "${#sys_icon_present[@]}" -gt 0 ] || [ "${#sys_data_present[@]}" -gt 0 ] ||
        [ "${#sys_theme_present[@]}" -gt 0 ] || [ "${#sys_model_present[@]}" -gt 0 ]
}

user_found() {
    [ "${#user_lib_present[@]}" -gt 0 ] || [ "${#user_conf_present[@]}" -gt 0 ] ||
        [ "${#user_icon_present[@]}" -gt 0 ] || [ "${#user_data_present[@]}" -gt 0 ] ||
        [ "${#user_theme_present[@]}" -gt 0 ] || [ "${#user_model_present[@]}" -gt 0 ] ||
        [ "${#user_env_present[@]}" -gt 0 ] || [ "${#user_option_present[@]}" -gt 0 ] ||
        [ "${#user_learning_present[@]}" -gt 0 ] || [ "${#user_conf_file_present[@]}" -gt 0 ]
}

user_data_found() {
    [ "${#user_option_present[@]}" -gt 0 ] || [ "${#user_learning_present[@]}" -gt 0 ] ||
        [ "${#user_conf_file_present[@]}" -gt 0 ]
}

# ---------------------------------------------------------------- 计划删除清单（--dry-run）

# 逐行打印「计划删除清单」（契约见文件头）。清单是静态的：按固定落点与两份清单给出，不按探测
# 结果过滤——干净机器上也要能取到完整的可卸载集合，守卫拿它比对安装集合。
print_plan() {
    local p
    for p in "${sys_lib_patterns[@]}" "${sys_conf_files[@]}" "${sys_icon_files[@]}"; do
        printf '%s\n' "- $p"
    done
    for p in "${sys_data[@]}"; do printf '%s\n' "- $p"; done
    for p in "${sys_themes[@]}"; do printf '%s\n' "- $p/"; done
    for p in "$user_lib" "${user_conf_files[@]}" "${user_icon_files[@]}"; do
        printf '%s\n' "- $p"
    done
    for p in "${user_data[@]}"; do printf '%s\n' "- $p"; done
    for p in "${user_themes[@]}"; do printf '%s\n' "- $p/"; done
    printf '%s\n' "- $user_env_file"
    printf '%s\n' "? $sys_models_dir/*.bin" "? $user_models_dir/*.bin"
    printf '%s\n' "? $engine_hux_dir/tiger_sentence.options.yaml" "? $engine_hux_dir/user.yaml"
    printf '%s\n' "? $engine_hux_dir/tiger_sentence_learning_*.userdb*"
    printf '%s\n' "? $engine_conf"
}

# ---------------------------------------------------------------- ① 探测安装

report_probe() {
    if sys_found; then
        info "  系统级：插件库 ${#sys_lib_present[@]} 项、配置 ${#sys_conf_present[@]} 项、图标 ${#sys_icon_present[@]} 个、随包数据 ${#sys_data_present[@]} 项、主题 ${#sys_theme_present[@]} 套、模型 ${#sys_model_present[@]} 个"
    else
        note "  系统级：未发现安装（跳过）"
    fi
    if user_found; then
        info "  用户级：插件库 ${#user_lib_present[@]} 项、配置 ${#user_conf_present[@]} 项、图标 ${#user_icon_present[@]} 个、随包数据 ${#user_data_present[@]} 项、主题 ${#user_theme_present[@]} 套、模型 ${#user_model_present[@]} 个、environment.d ${#user_env_present[@]} 项"
    else
        note "  用户级：未发现安装（跳过）"
    fi
}

# ---------------------------------------------------------------- ② 确认可选项

# 主题 / 模型 / 用户数据三项都靠问答决定（只问存在的项）：主题缺省删，模型与用户数据缺省留。
ask_optional() {
    if [ "$dry_run" -eq 1 ]; then
        note "  （--dry-run 不提问：主题删，模型与用户数据按缺省保留）"
        return 0
    fi
    if [ "${#sys_theme_present[@]}" -gt 0 ] || [ "${#user_theme_present[@]}" -gt 0 ]; then
        if ask "是否卸载共享主题（$(( ${#sys_theme_present[@]} + ${#user_theme_present[@]} )) 套）？" y; then remove_themes=1; else remove_themes=0; fi
    fi
    if [ "${#sys_model_present[@]}" -gt 0 ] || [ "${#user_model_present[@]}" -gt 0 ]; then
        if ask "是否卸载模型（$(( ${#sys_model_present[@]} + ${#user_model_present[@]} )) 个 .bin，体积大、可复用）？" n; then remove_models=1; else remove_models=0; fi
    fi
    if user_data_found; then
        if ask "是否删除用户数据（选项 / 学习库 / conf/hux.conf）？" n; then remove_user_data=1; else remove_user_data=0; fi
    fi
}

# ---------------------------------------------------------------- ③ 移除

remove_system() {
    if ! sys_found; then return 0; fi
    local -a themes=() models=()
    if [ "$remove_themes" -eq 1 ]; then themes=("${sys_theme_present[@]}"); fi
    if [ "$remove_models" -eq 1 ]; then models=("${sys_model_present[@]}"); fi
    note "  系统级（需要 sudo）："
    need_sudo=1
    remove_abs "${sys_lib_present[@]}" "${sys_conf_present[@]}" "${sys_icon_present[@]}" "${sys_data_present[@]}"
    remove_abs "${themes[@]}"
    remove_abs "${models[@]}"
    cleanup_empty_dirs "$sys_themes_dir" "$sys_models_dir" "$sys_hux_dir"
}

remove_user() {
    if ! user_found; then return 0; fi
    local -a themes=() models=()
    if [ "$remove_themes" -eq 1 ]; then themes=("${user_theme_present[@]}"); fi
    if [ "$remove_models" -eq 1 ]; then models=("${user_model_present[@]}"); fi
    note "  用户级："
    need_sudo=0
    remove_abs "${user_lib_present[@]}" "${user_conf_present[@]}" "${user_icon_present[@]}" \
        "${user_data_present[@]}" "${user_env_present[@]}"
    remove_abs "${themes[@]}"
    remove_abs "${models[@]}"
    if [ "$remove_user_data" -eq 1 ]; then
        remove_abs "${user_option_present[@]}" "${user_learning_present[@]}" "${user_conf_file_present[@]}"
        if [ "$remove_models" -eq 1 ] && [ -d "$engine_hux_dir" ]; then
            # 模型也删：用户数据目录整棵端掉（含未被清单覆盖的残留）。
            remove_abs "$engine_hux_dir"
        fi
    fi
    cleanup_empty_dirs "$user_themes_dir" "$user_models_dir" "$engine_hux_dir" "$(dirname "$user_env_file")"
}

# ---------------------------------------------------------------- ④ 结果清单

report_leftover() {
    if [ -d "$1" ] && [ -n "$(find "$1" -type f -print -quit 2>/dev/null)" ]; then
        kept+=("$1/ 仍在（仍有清单之外的文件：自取模型或自建文件）")
    fi
}

report_result() {
    removed=()
    kept=()
    absent=()
    local line
    if [ "$dry_run" -eq 0 ]; then
        verb_removed="已卸载"
        verb_kept="未卸载"
    else
        verb_removed="将卸载"
        verb_kept="将保留"
    fi
    if sys_found; then
        if [ "${#sys_lib_present[@]}" -gt 0 ]; then removed+=("系统级插件库：$(show_paths "${sys_lib_present[@]}")"); fi
        if [ "${#sys_conf_present[@]}" -gt 0 ]; then removed+=("系统级配置：$(show_paths "${sys_conf_present[@]}")"); fi
        if [ "${#sys_icon_present[@]}" -gt 0 ]; then removed+=("系统级图标：${#sys_icon_present[@]} 个（/usr/share/icons/hicolor/）"); fi
        if [ "${#sys_data_present[@]}" -gt 0 ]; then removed+=("系统级随包数据：${#sys_data_present[@]} 项（$sys_hux_dir/）"); fi
        if [ "$remove_themes" -eq 1 ] && [ "${#sys_theme_present[@]}" -gt 0 ]; then
            removed+=("系统级主题：${#sys_theme_present[@]} 套（$sys_themes_dir/）")
        fi
        if [ "$remove_models" -eq 1 ] && [ "${#sys_model_present[@]}" -gt 0 ]; then
            removed+=("系统级模型：${#sys_model_present[@]} 个（$(show_paths "${sys_model_present[@]}")）")
        fi
    else
        absent+=("系统级安装（未发现，跳过）")
    fi
    if user_found; then
        if [ "${#user_lib_present[@]}" -gt 0 ]; then removed+=("用户级插件库：$(show_paths "${user_lib_present[@]}")"); fi
        if [ "${#user_conf_present[@]}" -gt 0 ]; then removed+=("用户级配置：$(show_paths "${user_conf_present[@]}")"); fi
        if [ "${#user_icon_present[@]}" -gt 0 ]; then removed+=("用户级图标：${#user_icon_present[@]} 个（$(show_paths "$user_share/icons/hicolor")/）"); fi
        if [ "${#user_data_present[@]}" -gt 0 ]; then removed+=("用户级随包数据：${#user_data_present[@]} 项（$(show_paths "$user_hux_dir")/）"); fi
        if [ "$remove_themes" -eq 1 ] && [ "${#user_theme_present[@]}" -gt 0 ]; then
            removed+=("用户级主题：${#user_theme_present[@]} 套（$(show_paths "$user_themes_dir")/）")
        fi
        if [ "$remove_models" -eq 1 ] && [ "${#user_model_present[@]}" -gt 0 ]; then
            removed+=("用户级模型：${#user_model_present[@]} 个（$(show_paths "${user_model_present[@]}")）")
        fi
        if [ "${#user_env_present[@]}" -gt 0 ]; then removed+=("用户级环境变量文件：$(show_paths "${user_env_present[@]}")"); fi
        if [ "$remove_user_data" -eq 1 ]; then
            if [ "${#user_option_present[@]}" -gt 0 ]; then removed+=("用户数据（选项）：$(show_paths "${user_option_present[@]}")"); fi
            if [ "${#user_learning_present[@]}" -gt 0 ]; then removed+=("用户数据（学习库）：$(show_paths "${user_learning_present[@]}")"); fi
            if [ "${#user_conf_file_present[@]}" -gt 0 ]; then removed+=("用户数据（配置页设置）：$(show_paths "${user_conf_file_present[@]}")"); fi
        fi
    else
        absent+=("用户级安装（未发现，跳过）")
    fi
    # 未卸载项：逐条给出原因。
    if [ "$remove_themes" -eq 0 ]; then
        if [ "${#sys_theme_present[@]}" -gt 0 ] || [ "${#user_theme_present[@]}" -gt 0 ]; then
            kept+=("共享主题（$(( ${#sys_theme_present[@]} + ${#user_theme_present[@]} )) 套）：按确认结果保留（要删请重跑并在「是否卸载共享主题」一问回答 y）")
        fi
    fi
    if [ "$remove_models" -eq 0 ]; then
        if [ "${#sys_model_present[@]}" -gt 0 ]; then
            kept+=("系统级模型：$(show_paths "${sys_model_present[@]}")（体积大、可复用；要删请重跑并在「是否卸载模型」一问回答 y）")
        fi
        if [ "${#user_model_present[@]}" -gt 0 ]; then
            kept+=("用户级模型：$(show_paths "${user_model_present[@]}")（体积大、可复用；要删请重跑并在「是否卸载模型」一问回答 y）")
        fi
    fi
    if [ "$remove_user_data" -eq 0 ] && user_data_found; then
        kept+=("用户数据：$(show_paths "$engine_hux_dir" "$engine_conf")（选项 / 学习库 / 配置页设置；要删请重跑并在「是否删除用户数据」一问回答 y）")
    fi
    if [ "$dry_run" -eq 0 ]; then
        report_leftover "$sys_hux_dir"
        report_leftover "$engine_hux_dir"
    fi

    printf '\n'
    info "卸载结果"
    if [ "${#removed[@]}" -gt 0 ]; then
        info "  $verb_removed："
        for line in "${removed[@]}"; do info "    · $line"; done
    else
        note "  （没有可卸载的项）"
    fi
    if [ "${#kept[@]}" -gt 0 ]; then
        info "  $verb_kept："
        for line in "${kept[@]}"; do info "    · $line"; done
    fi
    if [ "${#absent[@]}" -gt 0 ]; then
        note "  未发现："
        for line in "${absent[@]}"; do note "    · $line"; done
    fi
}

print_tail() {
    if [ "$dry_run" -eq 0 ]; then
        printf '\n'
        info "请重启 fcitx5 让卸载生效："
        green "    nohup fcitx5 -r -d >/dev/null 2>&1 &"
    fi
    printf '\n'
    white_url "  建议：如有任何改进建议，欢迎在此留痕：" "https://github.com/crrvx/hux-ime/issues"
    white "  感谢使用与收藏。"
}

# ---------------------------------------------------------------- 主流程

if [ "$(id -u)" -eq 0 ]; then die "请以普通用户运行（脚本会在需要时调用 sudo）。"; fi

# 可选项的初始答案：主题删、模型留、用户数据留；交互问答会按回答改写，--dry-run 不提问即用缺省。
remove_themes=1
remove_models=0
remove_user_data=0

sys_lib_present=() sys_conf_present=() sys_icon_present=() sys_data_present=()
sys_theme_present=() sys_model_present=()
user_lib_present=() user_conf_present=() user_icon_present=() user_data_present=()
user_theme_present=() user_model_present=() user_env_present=()
user_option_present=() user_learning_present=() user_conf_file_present=()
need_sudo=0

step "①探测安装"
probe
report_probe
step "②确认可选项"
ask_optional
step "③移除"
remove_system
remove_user
step "④结果清单"
report_result
if [ "$dry_run" -eq 1 ]; then
    printf '\n'
    note "计划删除清单（每行「<标记> <绝对路径>」：- 按缺省会删；? 回答 y 才删）："
    print_plan
fi
print_tail

if [ "$dry_run" -eq 1 ]; then
    printf '\n'
    note "（--dry-run：以上命令均未实际执行）"
fi
