#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 探针类金样生成器的共用契约（被 `source` 引入，不单独执行；调用方已 `set -euo pipefail`）：
# 夹具护栏、pin 版 Lua 模块清单、最小共享数据、方案补丁、合成小码表、librime-lua 插件探测、
# 写库骨架（断言 + 确定性 gzip + 原子替换）。
# 使用方：gen_key_golden.sh / gen_key_sequence_golden.sh /
#         gen_key_sequence_tab_golden.sh / gen_sound_to_char_shape_golden.sh
# 前置变量：取 pin 版文件（pin_show / copy_lua_modules）者须定义 REF / PIN。
# 退出码：0 成功；1 契约不满足（夹具不符、缺插件、产物残缺）。

# 夹具护栏：入库夹具（`goldens/<组>/`）不得被生成器当副作用重写。
# 只做「逐字节比对」这一件事（不删传入文件——它可能是 pin 工作区里的真实文件）：
# 一致才继续，入库文件保持原样不落盘；不一致即失败，并区分两种成因：
# 上游 pin 变化（须同步更新金样与夹具）或夹具漂移（应还原）。
# 用法：guard_fixture <本次生成的临时产物> <入库文件> <说明>
guard_fixture() {
    local staged="$1" committed="$2" what="$3"
    if [ ! -f "$committed" ]; then
        echo "生成失败：入库夹具缺失：$committed（$what）" >&2
        exit 1
    fi
    if ! cmp -s "$staged" "$committed"; then
        echo "生成失败：$what 与入库夹具不一致（护栏拦下，未写入任何入库文件）" >&2
        echo "  入库：$committed  sha256 $(sha256sum "$committed" | cut -d' ' -f1)" >&2
        echo "  本次：$staged  sha256 $(sha256sum "$staged" | cut -d' ' -f1)" >&2
        echo "  成因二选一：①参照 pin ${PIN:-（未声明）} 的对应源文件已变（上游推进）——须同步更新金样与夹具，" >&2
        echo "  不能由生成器静默覆盖；②入库夹具被本地改动（夹具漂移）——应还原夹具。" >&2
        exit 1
    fi
    echo "夹具一致（入库文件未改写）：$what -> $committed" >&2
}

# 取 pin 版单文件（内容与已入库金样同一参照修订）。
# 用法：pin_show <仓库内相对路径> <目标文件>
pin_show() {
    git -C "$REF" show "$PIN:$1" > "$2"
}

# 5 件 pin 版 Lua 模块名（一行一个）。
lua_module_list() {
    printf '%s\n' \
        tiger_sentence.lua \
        tiger_sentence_learning.lua \
        tiger_sentence_ngram.lua \
        tiger_sentence_cache.lua \
        tiger_sentence_lexical.lua
}

# 把 5 件 Lua 模块取到 <目标目录>/lua/：给出 <源目录> 时从该目录 `cp`（它已是 pin 树），
# 否则经 `git show` 从 pin 取。
# 用法：copy_lua_modules <目标目录> [<源目录>]
copy_lua_modules() {
    local dest="$1/lua" src="${2:-}" name
    mkdir -p "$dest"
    for name in $(lua_module_list); do
        if [ -n "$src" ]; then
            cp "$src/lua/$name" "$dest/$name"
        else
            pin_show "lua/$name" "$dest/$name"
        fi
    done
}

# 最小共享数据（与参照集成测试同构；页大小与重放侧 `DEFAULT_PAGE_SIZE` 一致）。
# 用法：default_yaml <目标文件>
default_yaml() {
    cat > "$1" <<'YAML'
config_version: "1.0"
schema_list:
  - schema: tiger_sentence
menu:
  page_size: 5
recognizer:
  patterns: {}
YAML
}

# 方案补丁（高频繁用限制关闭 + tab_learning 开关）：写入 <目录>/tiger_sentence.custom.yaml。
# 用法：custom_yaml <目录> <false|true>
custom_yaml() {
    printf 'patch:\n  tiger_sentence/high_freq_limit: 0\n  tiger_sentence/tab_learning: %s\n' \
        "$2" > "$1/tiger_sentence.custom.yaml"
}

# 合成 27 行小码表（单字/词组 + 受控重码，覆盖空码自动上屏与 Tab 翻页路径）。
# 码表写 <码表目录>/tiger_sentence.codes.txt，方案补丁写 <补丁目录>（两者可同日录不同目录）。
# 用法：synth_code_table <码表目录> <补丁目录> <false|true>
synth_code_table() {
    python3 - "$1" "$2" "$3" <<'PY'
import pathlib
import sys

codes_dir = pathlib.Path(sys.argv[1])
patch_dir = pathlib.Path(sys.argv[2])
patch_dir.mkdir(parents=True, exist_ok=True)
table = ["刘\tvp", "甲\tab", "乙\tab", "一\tcd", "第7\tz"]
table += [f"{chr(0x4E00 + i)}\tja" for i in range(22)]
(codes_dir / "tiger_sentence.codes.txt").write_text("\n".join(table) + "\n", encoding="utf-8")
(patch_dir / "tiger_sentence.custom.yaml").write_text(
    "patch:\n  tiger_sentence/high_freq_limit: 0\n"
    f"  tiger_sentence/tab_learning: {sys.argv[3]}\n",
    encoding="utf-8")
PY
}

# librime-lua 插件路径（LUA_PLUGIN 可覆盖）写 stdout；缺失即显式失败退出
# （`set -e` 下裸 `test -f` 会静默退出，无从诊断）。
# 用法：plugin="$(require_lua_plugin)"
require_lua_plugin() {
    local plugin="${LUA_PLUGIN:-/usr/lib/rime-plugins/librime-lua.so}"
    if [ ! -f "$plugin" ]; then
        echo "生成失败：缺少 librime-lua 插件：$plugin（可用 LUA_PLUGIN 覆盖）" >&2
        exit 1
    fi
    printf '%s\n' "$plugin"
}

# 写库前断言：<文件> 至少 1 行匹配 <grep 基本正则>，否则显式失败（入库金样保持原样）。
# 用法：golden_require <文件> <grep 模式> <说明>
golden_require() {
    if grep -q "$2" "$1" 2>/dev/null; then
        return 0
    fi
    echo "生成失败：$3（$1 不含匹配 '$2' 的记录；输入为空或全为注释？）" >&2
    exit 1
}

# 原子写库：确定性 gzip（`-n` 去文件名与时间戳，同一输入必得同一字节）写 `$2.tmp.$$` 后 `mv`。
# 用法：write_golden <临时 TSV> <入库 .tsv.gz>
write_golden() {
    gzip -9 -n -c "$1" > "$2.tmp.$$"
    mv "$2.tmp.$$" "$2"
}
