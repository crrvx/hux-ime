#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# install.sh / uninstall.sh 的公共片段：颜色与输出助手、命令执行、交互问答、清单解析。
# 由两个脚本 `source` 进来（调用方先自己 `set -euo pipefail`）。本文件只定义变量与函数：
# 不打印任何东西、不读参数、不改调用方的 cwd，故也不能单独执行。
#
# 需要 bash ≥ 4.4：`mapfile` 与关联数组自 4.0 起就有，但 `set -u` 下展开空数组
# （`"${arr[@]}"`）到 4.4 才不再算未绑定——两个脚本都靠这条（如「没有已存在的项」时的
# `remove_abs "${paths[@]}"`），故下限取 4.4，低于此版本在下方守卫处直接退出。

if [ -z "${BASH_VERSINFO:-}" ] ||
    [ "${BASH_VERSINFO[0]}" -lt 4 ] ||
    { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -lt 4 ]; }; then
    printf '%s\n' '需要 bash ≥ 4.4（用到 mapfile 与关联数组；macOS 自带的 bash 3.2 不行）。' >&2
    exit 1
fi

# ---------------------------------------------------------------- 颜色与输出

# 颜色只在终端上给出：非 TTY（重定向、管道、CI 日志）、设了 NO_COLOR，或显式 `plain`
# （预览的「计划删除清单」要机器可解析）时退化为纯文本。
# 调用时机由调用方定：解析参数前先来一次 `auto`，报错台词也有色；解析出 `--dry-run`
# 之类的「必须纯文本」后再来一次 `plain` 覆盖。
setup_colors() {
    if [ "${1:-auto}" = auto ] && [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
        c_orange=$'\033[38;5;208m' # 一般文字
        c_green=$'\033[32m'        # 网址 / 命令
        c_gray=$'\033[38;5;245m'   # 注释 / 次要说明
        c_white=$'\033[97m'        # 建议 / 声明 / 感谢
        c_cyan=$'\033[36m'         # 许可证名（GPL-3.0-or-later）
        c_reset=$'\033[0m'
    else
        c_orange='' c_green='' c_gray='' c_white='' c_cyan='' c_reset=''
    fi
}

# 颜色变量先置空：`setup_colors` 之前若有输出（或忘了调用），也退化为纯文本。
c_orange='' c_green='' c_gray='' c_white='' c_cyan='' c_reset=''

info() { printf '%s%s%s\n' "$c_orange" "$*" "$c_reset"; } # 一般文字
green() { printf '%s%s%s\n' "$c_green" "$*" "$c_reset"; } # 网址 / 命令
note() { printf '%s%s%s\n' "$c_gray" "$*" "$c_reset"; }   # 注释 / 次要说明
white() { printf '%s%s%s\n' "$c_white" "$*" "$c_reset"; } # 建议 / 声明 / 感谢

# 段落标题：与前文隔一个空行（只差这个空行，故复用 info）。
step() {
    printf '\n'
    info "$@"
}

die() {
    printf '%s%s%s\n' "$c_orange" "$*" "$c_reset" >&2
    exit 1
}

# 白色正文 + 绿色网址（建议 / 声明的同一行）。
white_url() {
    printf '%s%s%s%s%s%s\n' "$c_white" "$1" "$c_reset" "$c_green" "$2" "$c_reset"
}

# 白色正文 + 青色许可证名 + 白色续文（声明行内混色，安装器的结尾声明用）。
white_license() {
    printf '%s%s%s%s%s%s%s\n' "$c_white" "$1" "$c_cyan" "$2" "$c_white" "$3" "$c_reset"
}

# 执行命令；--dry-run 只打印（绿色 `+` 前缀）。读调用方的 dry_run：0 = 真跑。
run() {
    printf '%s+' "$c_green"
    printf ' %q' "$@"
    printf '%s\n' "$c_reset"
    if [ "$dry_run" -eq 0 ]; then
        "$@"
    fi
}

# ---------------------------------------------------------------- 交互问答

# 提问：提示里的缺省答案标在 [Y/n] / [y/N]；标准输入读不到时按缺省处理。
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

# ---------------------------------------------------------------- 清单

# 读一份清单，滤掉空行与 `#` 注释行（注释不是条目）。
manifest_lines() {
    local line
    while IFS= read -r line; do
        case "$line" in '' | '#'*) continue ;; esac
        printf '%s\n' "$line"
    done <"$1"
}

# 清单文件必须存在（缺了既装不上、也卸不干净）：<路径> <清单名>。
manifest_require() {
    if [ ! -f "$1" ]; then die "缺少 $1（$2）"; fi
}

# 清单必须有有效行：<路径> <清单名> <已读条数>。
manifest_require_lines() {
    if [ "$3" -eq 0 ]; then die "$1 没有有效行（$2缺失或为空）"; fi
}
