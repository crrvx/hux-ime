#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 虎虚（hux-ime）卸载：①探测安装 ②确认可选项 ③移除 ④结果清单。
# 用法与选项的唯一出处是下面的 `usage()`（`--help` 打的就是它，不要再往这里抄一份）。
# 全交互：先探测系统级与用户级两处安装，只对存在的项提问与操作；主题缺省卸载，
# 模型（体积大、可复用）与用户数据缺省保留。卸载结束不自动重启 fcitx5。
# 公共片段（颜色 / 输出 / 执行 / 清单解析）见 tools/scripts/lib.sh；需要 bash ≥ 4.4。
#
# `--dry-run` 的「计划删除清单」契约（机器可解析；守卫 tools/checks/check_uninstall_clean.py 按它解析）：
#   每行 `<标记> <绝对路径>`，标记两种：`-` = 按缺省会删；`?` = 需交互确认（回答 y）后才删，
#   即模型 / 用户数据这类缺省保留的项。
#   路径为绝对路径，一行一条；整个目录以 `/` 结尾；通配用 `*`（如 multiarch 的 addon 目录）。
#   清单按固定落点与两份清单静态给出、不按探测结果过滤：干净机器上也能取到完整的可卸载集合。
#   其余人读信息照常打印，守卫只认上述两种前缀行。
#   ——这一段是规范文本：`usage()` 与 `print_plan()` 只作面向用户的转述，要改契约就改这里。
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

# ---------------------------------------------------------------- 公共片段

# 颜色 / 输出 / 执行 / 清单解析由两个脚本共用（bash 版本守卫也在该文件里）。
. tools/scripts/lib.sh

# 颜色先按终端与 NO_COLOR 落定：解析参数前就要定，未知参数的报错也得有色。
setup_colors auto

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
    # 契约的规范文本在文件头；这里只是给人看的转述（改契约时以文件头为准）。
    printf '%s\n' '  --dry-run 的「计划删除清单」契约：每行 `<标记> <绝对路径>`，标记 - 表示按缺省'
    printf '%s\n' '  会删、? 表示回答 y 才删（模型 / 用户数据）；绝对路径一行一条，整个目录以 / 结尾，'
    printf '%s\n' '  通配用 *（如 multiarch 的 addon 目录）。'
}

# ---------------------------------------------------------------- 参数

dry_run=0

for arg in "$@"; do
    case "$arg" in
    --dry-run)
        dry_run=1
        # 预览的「计划删除清单」要机器可解析（守卫按行首两字符解析）：解析到这一步就落定
        # 纯文本（不再事先预扫一遍 "$@"），后面的帮助与报错台词也跟着变纯文本。
        setup_colors plain
        ;;
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

# 两级的目录落点；插件库 / 配置 / 图标 / 随包数据 / 模型的具体路径都在下面的落点表里。
sys_themes_dir=/usr/share/fcitx5/themes
sys_hux_dir=/usr/share/fcitx5/hux
sys_models_dir="$sys_hux_dir/models"
user_themes_dir="$user_share/fcitx5/themes"
user_hux_dir="$user_share/fcitx5/hux"
user_models_dir="$user_hux_dir/models"

# ---------------------------------------------------------------- 清单（与 CMake / install.sh 同源）

manifest_require data/MANIFEST 随包数据清单
manifest_require assets/themes/MANIFEST 共享主题清单
data_names=()
while IFS= read -r entry; do data_names+=("$(basename "$entry")"); done < <(manifest_lines data/MANIFEST)
manifest_require_lines data/MANIFEST 随包数据清单 "${#data_names[@]}"
theme_names=()
while IFS= read -r entry; do theme_names+=("$entry"); done < <(manifest_lines assets/themes/MANIFEST)
manifest_require_lines assets/themes/MANIFEST 共享主题清单 "${#theme_names[@]}"

# <前缀> <名字…> → `;` 分隔的绝对路径串（落点表的「来源」字段用；路径里不会出现 `;`）。
absolute_list() {
    local prefix=$1 name out=''
    shift
    for name in "$@"; do
        if [ -n "$out" ]; then out="$out;"; fi
        out="$out$prefix/$name"
    done
    printf '%s' "$out"
}

sys_data_src=$(absolute_list "$sys_hux_dir" "${data_names[@]}")
user_data_src=$(absolute_list "$user_hux_dir" "${data_names[@]}")
sys_theme_src=$(absolute_list "$sys_themes_dir" "${theme_names[@]}")
user_theme_src=$(absolute_list "$user_themes_dir" "${theme_names[@]}")

# ---------------------------------------------------------------- 落点表

# 卸载涉及的每一项落点，一行一项、`|` 分九列：
#   ① 作用域 sys|user   ② 键（探测与结果按 `作用域/键` 索引）
#   ③ 来源：`;` 分隔的路径 / 通配模式（探测取存在的；计划清单逐条原样打印）
#   ④ 量词（结果行的 N 项 / 个 / 套）   ⑤ 探测概览里的短名
#   ⑥ 结果行标签   ⑦ 取值方式 list|count_dir|count_list   ⑧ 取值参数（count_dir 的父目录）
#   ⑨ 分组：core / themes 按缺省删，models / userdata 缺省保留（由问答决定，见 group_selected）
# 行的顺序 = ①探测概览、④结果清单、--dry-run 计划清单的行序，改顺序就是改用户可见行序。
targets=(
    # 系统级插件库兼容 lib、lib64 与 multiarch（Debian/Ubuntu：lib/<triplet>/fcitx5）。
    "sys|lib|/usr/lib/fcitx5/libhux.so;/usr/lib64/fcitx5/libhux.so;/usr/lib/*/fcitx5/libhux.so|项|插件库|系统级插件库|list||core"
    "sys|conf|/usr/share/fcitx5/addon/hux.conf;/usr/share/fcitx5/inputmethod/hux.conf|项|配置|系统级配置|list||core"
    "sys|icon|/usr/share/icons/hicolor/scalable/apps/hux.svg;/usr/share/icons/hicolor/48x48/apps/hux.png;/usr/share/icons/hicolor/22x22/apps/hux.png|个|图标|系统级图标|count_dir|/usr/share/icons/hicolor|core"
    "sys|data|$sys_data_src|项|随包数据|系统级随包数据|count_dir|$sys_hux_dir|core"
    "sys|theme|$sys_theme_src|套|主题|系统级主题|count_dir|$sys_themes_dir|themes"
    "sys|model|$sys_models_dir/*.bin|个|模型|系统级模型|count_list||models"
    "user|lib|$user_prefix/lib/fcitx5/libhux.so|项|插件库|用户级插件库|list||core"
    "user|conf|$user_share/fcitx5/addon/hux.conf;$user_share/fcitx5/inputmethod/hux.conf|项|配置|用户级配置|list||core"
    "user|icon|$user_share/icons/hicolor/scalable/apps/hux.svg;$user_share/icons/hicolor/48x48/apps/hux.png;$user_share/icons/hicolor/22x22/apps/hux.png|个|图标|用户级图标|count_dir|$user_share/icons/hicolor|core"
    "user|data|$user_data_src|项|随包数据|用户级随包数据|count_dir|$user_hux_dir|core"
    "user|theme|$user_theme_src|套|主题|用户级主题|count_dir|$user_themes_dir|themes"
    "user|model|$user_models_dir/*.bin|个|模型|用户级模型|count_list||models"
    "user|env|$user_env_file|项|environment.d|用户级环境变量文件|list||core"
    # 学习库是目录 `<方案 id 哈希>.userdb/`（LevelDB）；探测用同一模式。
    "user|option|$engine_hux_dir/tiger_sentence.options.yaml;$engine_hux_dir/user.yaml|||用户数据（选项）|list||userdata"
    "user|learning|$engine_hux_dir/tiger_sentence_learning_*.userdb|||用户数据（学习库）|list||userdata"
    "user|conf_file|$engine_conf|||用户数据（配置页设置）|list||userdata"
)

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

# 已存在路径的连续存放区：present_paths 按落点表顺序连续放着各项探测到的路径，
# present_start / present_count 记每项在其中的切片（关联数组，键为 `作用域/键`）。
present_paths=()
declare -A present_start=()
declare -A present_count=()

# 探测存在的项（交互问答、结果清单与真正的删除都基于它；--dry-run 的计划清单是静态的）。
probe() {
    local record s key sources rest group start=0
    local -a patterns present
    present_paths=()
    present_start=()
    present_count=()
    for record in "${targets[@]}"; do
        IFS='|' read -r s key sources _ _ _ _ _ group <<<"$record"
        IFS=';' read -r -a patterns <<<"$sources"
        mapfile -t present < <(expand_existing "${patterns[@]}" | dedupe_paths)
        present_start["$s/$key"]=$start
        present_count["$s/$key"]=${#present[@]}
        present_paths+=("${present[@]}")
        start=$((start + ${#present[@]}))
    done
}

# 某落点是否探测到路径（键为 `作用域/键`）。
present_any() {
    [ "${present_count[$1]}" -gt 0 ]
}

# 某落点已存在的路径（每行一条）；没有时什么都不输出——直接展开空数组会多出一个空行。
present_list() {
    local -a p=("${present_paths[@]:${present_start[$1]}:${present_count[$1]}}")
    if [ "${#p[@]}" -gt 0 ]; then printf '%s\n' "${p[@]}"; fi
}

# 某作用域是否探测到任何项（可限定分组：`userdata` 只问用户数据那三项）。
scope_found() {
    local record s key rest group
    for record in "${targets[@]}"; do
        IFS='|' read -r s key _ _ _ _ _ _ group <<<"$record"
        if [ "$s" != "$1" ]; then continue; fi
        if [ -n "${2:-}" ] && [ "$group" != "$2" ]; then continue; fi
        if present_any "$s/$key"; then return 0; fi
    done
    return 1
}

# 作用域的中文名（探测概览与「未发现」行）。
scope_label() {
    case "$1" in
    sys) printf '%s' 系统级 ;;
    user) printf '%s' 用户级 ;;
    esac
}

# 该分组此刻是否要删：核心项总是删；主题 / 模型 / 用户数据看问答结果（缺省：主题删，
# 模型与用户数据留——它们在计划清单里因此是 `?` 行）。
group_selected() {
    case "$1" in
    core) ;;
    themes) [ "$remove_themes" -eq 1 ] || return 1 ;;
    models) [ "$remove_models" -eq 1 ] || return 1 ;;
    userdata) [ "$remove_user_data" -eq 1 ] || return 1 ;;
    *) return 1 ;;
    esac
    return 0
}

# 删除某作用域：按分组各合成一次 rm（分组内的路径顺序 = 落点表顺序）。
remove_scope() {
    local scope=$1 g record s key rest group
    local -a paths
    for g in core themes models userdata; do
        if ! group_selected "$g"; then continue; fi
        paths=()
        for record in "${targets[@]}"; do
            IFS='|' read -r s key _ _ _ _ _ _ group <<<"$record"
            if [ "$s" = "$scope" ] && [ "$group" = "$g" ]; then
                paths+=("${present_paths[@]:${present_start[$s/$key]}:${present_count[$s/$key]}}")
            fi
        done
        remove_abs "${paths[@]}"
    done
}

remove_system() {
    if ! scope_found sys; then return 0; fi
    note "  系统级（需要 sudo）："
    need_sudo=1
    remove_scope sys
    cleanup_empty_dirs "$sys_themes_dir" "$sys_models_dir" "$sys_hux_dir"
}

remove_user() {
    if ! scope_found user; then return 0; fi
    note "  用户级："
    need_sudo=0
    remove_scope user
    if [ "$remove_user_data" -eq 1 ] && [ "$remove_models" -eq 1 ] && [ -d "$engine_hux_dir" ]; then
        # 模型也删：用户数据目录整棵端掉（含未被清单覆盖的残留）。
        remove_abs "$engine_hux_dir"
    fi
    cleanup_empty_dirs "$user_themes_dir" "$user_models_dir" "$engine_hux_dir" "$(dirname "$user_env_file")"
}

# ---------------------------------------------------------------- 计划删除清单（--dry-run）

# 计划清单的两种行首标记（契约见文件头）：`-` 按缺省就删、`?` 回答 y 才删。
marker_delete='-'
marker_keep='?'

# 该分组在计划清单里的标记：模型 / 用户数据缺省保留，故回答 y 才删。
plan_marker() {
    case "$1" in
    models | userdata) printf '%s' "$marker_keep" ;;
    *) printf '%s' "$marker_delete" ;;
    esac
}

# 取该标记的行（来源逐条原样打印：清单是静态的，不按探测结果过滤；整目录以 `/` 结尾）。
plan_lines() {
    local wanted=$1 record s key sources rest group suffix p marker
    local -a patterns
    for record in "${targets[@]}"; do
        IFS='|' read -r s key sources _ _ _ _ _ group <<<"$record"
        marker=$(plan_marker "$group")
        if [ "$marker" != "$wanted" ]; then continue; fi
        suffix=''
        if [ "$key" = theme ]; then suffix='/'; fi
        IFS=';' read -r -a patterns <<<"$sources"
        for p in "${patterns[@]}"; do
            printf '%s\n' "$marker $p$suffix"
        done
    done
}

# 逐行打印「计划删除清单」（契约见文件头）：先 `-` 行、再 `?` 行。
print_plan() {
    note "计划删除清单（每行「<标记> <绝对路径>」：- 按缺省会删；? 回答 y 才删）："
    plan_lines "$marker_delete"
    plan_lines "$marker_keep"
}

# ---------------------------------------------------------------- ① 探测安装

# 探测概览的一行内容：「插件库 1 项、配置 2 项、…」；用户数据不是安装产物，不计入。
scope_summary() {
    local record s key rest noun short label group out=''
    for record in "${targets[@]}"; do
        IFS='|' read -r s key _ noun short _ _ _ group <<<"$record"
        if [ "$s" != "$1" ] || [ "$group" = userdata ]; then continue; fi
        if [ -n "$out" ]; then out="$out、"; fi
        out="$out$short ${present_count[$s/$key]} $noun"
    done
    printf '%s' "$out"
}

report_probe() {
    local scope
    for scope in sys user; do
        if scope_found "$scope"; then
            info "  $(scope_label "$scope")：$(scope_summary "$scope")"
        else
            note "  $(scope_label "$scope")：未发现安装（跳过）"
        fi
    done
}

# ---------------------------------------------------------------- ② 确认可选项

# 主题 / 模型 / 用户数据三项都靠问答决定（只问存在的项）：主题缺省删，模型与用户数据缺省留。
ask_optional() {
    if [ "$dry_run" -eq 1 ]; then
        note "  （--dry-run 不提问：主题删，模型与用户数据按缺省保留）"
        return 0
    fi
    local themes=$((present_count[sys/theme] + present_count[user/theme]))
    local models=$((present_count[sys/model] + present_count[user/model]))
    if [ "$themes" -gt 0 ]; then
        if ask "是否卸载共享主题（$themes 套）？" y; then remove_themes=1; else remove_themes=0; fi
    fi
    if [ "$models" -gt 0 ]; then
        if ask "是否卸载模型（$models 个 .bin，体积大、可复用）？" n; then remove_models=1; else remove_models=0; fi
    fi
    if scope_found user userdata; then
        if ask "是否删除用户数据（选项 / 学习库 / conf/hux.conf）？" n; then remove_user_data=1; else remove_user_data=0; fi
    fi
}

# ---------------------------------------------------------------- ④ 结果清单

report_leftover() {
    if [ -d "$1" ] && [ -n "$(find "$1" -type f -print -quit 2>/dev/null)" ]; then
        kept+=("$1/ 仍在（仍有清单之外的文件：自取模型或自建文件）")
    fi
}

# 已卸载 / 将卸载：按落点表顺序逐项给出（作用域 → 表序；没轮到的分组不计入）。
report_removed() {
    local scope record s key rest noun short label show arg group line
    local -a paths
    for scope in sys user; do
        if ! scope_found "$scope"; then
            absent+=("$(scope_label "$scope")安装（未发现，跳过）")
            continue
        fi
        for record in "${targets[@]}"; do
            IFS='|' read -r s key _ noun short label show arg group <<<"$record"
            if [ "$s" != "$scope" ]; then continue; fi
            if ! group_selected "$group"; then continue; fi
            if ! present_any "$s/$key"; then continue; fi
            mapfile -t paths < <(present_list "$s/$key")
            case "$show" in
            list) line=$(show_paths "${paths[@]}") ;;
            count_dir) line="${#paths[@]} $noun（$(show_paths "$arg")/）" ;;
            count_list) line="${#paths[@]} $noun（$(show_paths "${paths[@]}")）" ;;
            *) die "落点表的取值方式无法识别：$show" ;;
            esac
            removed+=("$label：$line")
        done
    done
}

# 「用户数据」保留行只列实际存在的落点：选项 / 学习库在引擎数据目录、配置页设置在 conf 文件，
# 只存在一部分时不再把两个目录都列出来。
userdata_dirs() {
    if present_any user/option || present_any user/learning; then
        printf '%s\n' "$engine_hux_dir"
    fi
    if present_any user/conf_file; then
        printf '%s\n' "$engine_conf"
    fi
}

# 未卸载项：逐条给出原因（只有缺省保留的三类会有：共享主题 / 模型 / 用户数据）。
report_kept() {
    local scope n
    local -a paths dirs
    if ! group_selected themes; then
        n=$((present_count[sys/theme] + present_count[user/theme]))
        if [ "$n" -gt 0 ]; then
            kept+=("共享主题（$n 套）：按确认结果保留（要删请重跑并在「是否卸载共享主题」一问回答 y）")
        fi
    fi
    if ! group_selected models; then
        for scope in sys user; do
            if ! present_any "$scope/model"; then continue; fi
            mapfile -t paths < <(present_list "$scope/model")
            kept+=("$(scope_label "$scope")模型：$(show_paths "${paths[@]}")（体积大、可复用；要删请重跑并在「是否卸载模型」一问回答 y）")
        done
    fi
    if ! group_selected userdata && scope_found user userdata; then
        mapfile -t dirs < <(userdata_dirs)
        kept+=("用户数据：$(show_paths "${dirs[@]}")（选项 / 学习库 / 配置页设置；要删请重跑并在「是否删除用户数据」一问回答 y）")
    fi
}

report_result() {
    removed=()
    kept=()
    absent=()
    local line verb_removed verb_kept
    if [ "$dry_run" -eq 0 ]; then
        verb_removed="已卸载"
        verb_kept="未卸载"
    else
        verb_removed="将卸载"
        verb_kept="将保留"
    fi
    report_removed
    report_kept
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
# need_sudo 由 remove_system / remove_user 在使用前设定；这里给初值，免得 set -u 下漏读。
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
    print_plan
fi
print_tail

if [ "$dry_run" -eq 1 ]; then
    printf '\n'
    note "（--dry-run：以上命令均未实际执行）"
fi
