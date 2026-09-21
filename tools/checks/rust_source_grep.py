#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""按「注释剥离后」的 Rust 源码做守卫匹配（复核整改第 4 批 C6/C7）。

CI 里原来的 `grep -RnE` 会命中**注释**，与 `docs/refactor.md` §7 的声明相反
（在 core 里写「为什么不能出现角色名 `"page_size"`」这类说明性注释也会让 CI 变红）。
本脚本先按 Rust 词法剥掉 `//`、`///`、`/* */`（含嵌套）与文档注释，再匹配；
字符串字面量原样保留，故「带引号的角色名 / 方案选项键字面量」照旧命中。

用法（默认：有命中即 exit 1）：

    # 1) 禁止出现（注释不算）
    python3 tools/checks/rust_source_grep.py --root crates/hux-core/src \\
        --pattern 'std::env::var|SystemTime|/usr/share|eprintln!|println!'
    # 2) 收集 + 白名单（多一行 / 少一行都失败）
    python3 tools/checks/rust_source_grep.py --root platform/fcitx5/src \\
        --pattern 'hux_scheme_tiger::[A-Za-z_:]+' \\
        --expect-set 'hux_scheme_tiger::scheme,hux_scheme_tiger::scheme::SCHEME_ID'
    # 3) 只允许集合内的值（`--split` 可把 `A, B` 拆开比较）
    python3 tools/checks/rust_source_grep.py --root platform/fcitx5/src \\
        --pattern 'hux_scheme_tiger::scheme::\\{([^{}]*)\\}' \\
        --expect-subset 'ASSETS,TigerScheme,SCHEME_ID' --split ','
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

RAW_PREFIX_RE = re.compile(r'(?:b?r|r)(#*)"')
CHAR_LITERAL_RE = re.compile(r"'(?:\\.|[^\\'])'")


def strip_comments(source: str) -> str:
    """把注释（`//`、`/* */` 含嵌套、文档注释）替换为空格，保留行号与列位置。

    字符串 / 字符字面量原样保留（含 `r"…"`、`r#"…"#`、`br"…"` 与转义）。
    """
    out = list(source)
    length = len(source)
    index = 0
    while index < length:
        char = source[index]
        if char == '"':
            index = _skip_string(source, index, out)
            continue
        if char == "'":
            match = CHAR_LITERAL_RE.match(source, index)
            if match:
                index = match.end()
                continue
            index += 1  # 生命周期（`'a`、`'static`）
            continue
        if char in "rRbB":
            match = RAW_PREFIX_RE.match(source, index)
            if match:
                index = _skip_raw_string(source, match.end(), match.group(1), out)
                continue
            index += 1
            continue
        if source.startswith("//", index):
            end = source.find("\n", index)
            end = length if end < 0 else end
            for position in range(index, end):
                out[position] = " "
            index = end
            continue
        if source.startswith("/*", index):
            depth = 1
            position = index + 2
            while position < length and depth:
                if source.startswith("/*", position):
                    depth += 1
                    position += 2
                elif source.startswith("*/", position):
                    depth -= 1
                    position += 2
                else:
                    position += 1
            for mask in range(index, min(position, length)):
                if source[mask] != "\n":
                    out[mask] = " "
            index = max(position, index + 2)
            continue
        index += 1
    return "".join(out)


def _skip_string(source: str, index: int, out: list[str]) -> int:
    """跳过普通字符串字面量（保留内容），返回结束引号之后的下标。"""
    position = index + 1
    length = len(source)
    while position < length:
        char = source[position]
        if char == "\\":
            position += 2
            continue
        if char == '"':
            return position + 1
        position += 1
    return length


def _skip_raw_string(source: str, position: int, hashes: str, out: list[str]) -> int:
    """跳过 `r#"…"#` 形式的原始字符串，返回结束分隔符之后的下标。"""
    del out  # 原始字符串内容原样保留（列位置不变），无需遮罩
    terminator = '"' + hashes
    end = source.find(terminator, position)
    if end < 0:
        return len(source)
    return end + len(terminator)


def collect_files(roots: list[Path]) -> list[Path]:
    files: list[Path] = []
    for root in roots:
        if root.is_file():
            files.append(root)
        else:
            files.extend(sorted(path for path in root.rglob("*.rs") if path.is_file()))
    return files


def mask_source(source: str, mode: str) -> str:
    if mode == "text":
        return source
    if mode == "no-comments":
        return strip_comments(source)
    raise SystemExit(f"rust_source_grep: 未知模式 {mode}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, action="append", required=True, help="目录或文件（可重复）")
    parser.add_argument("--pattern", required=True, help="Python 正则")
    parser.add_argument(
        "--mode",
        choices=("no-comments", "text"),
        default="no-comments",
        help="no-comments = 先剥离注释再匹配（默认）；text = 原样匹配（等同 grep）",
    )
    parser.add_argument("--expect-set", default=None, help="收集到的值必须**恰好**等于该集合（逗号分隔）")
    parser.add_argument(
        "--expect-subset", default=None, help="收集到的值必须都**属于**该集合（逗号分隔）"
    )
    parser.add_argument("--split", default=None, help="把每个收集值再按该分隔符拆开（去空白、去空项）")
    parser.add_argument("--label", default="", help="输出用的守卫名")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    pattern = re.compile(args.pattern)
    label = f"[{args.label}] " if args.label else ""
    files = collect_files([root.resolve() for root in args.root])
    if not files:
        print(f"rust_source_grep: {label}没扫到任何 .rs 文件（--root 写错了？）", file=sys.stderr)
        return 2

    matches: list[str] = []
    for path in files:
        text = path.read_text(encoding="utf-8")
        masked = mask_source(text, args.mode)
        for number, line in enumerate(masked.splitlines(), start=1):
            for found in pattern.finditer(line):
                value = found.group(1) if found.groups() else found.group(0)
                matches.append(value)
                print(f"{path}:{number}:{found.group(0).strip()}")

    if args.expect_set is None and args.expect_subset is None:
        if matches:
            print(
                f"rust_source_grep: {label}{len(matches)} 处命中（模式 {args.pattern}，mode={args.mode}）",
                file=sys.stderr,
            )
            return 1
        if args.verbose:
            print(f"rust_source_grep: {label}无命中（{len(files)} 个文件，mode={args.mode}）")
        return 0

    values = set(matches)
    if args.split:
        values = {
            piece.strip()
            for value in values
            for piece in value.split(args.split)
            if piece.strip()
        }
    expected = {
        item.strip() for item in (args.expect_set or args.expect_subset or "").split(",") if item.strip()
    }
    if args.expect_set is not None:
        extra = sorted(values - expected)
        missing = sorted(expected - values)
        if extra or missing:
            print(f"{label}白名单不符（期望恰好 {len(expected)} 项）", file=sys.stderr)
            for item in extra:
                print(f"  多出：{item}", file=sys.stderr)
            for item in missing:
                print(f"  缺失：{item}", file=sys.stderr)
            return 1
        if args.verbose:
            print(f"rust_source_grep: {label}白名单 {len(values)} 项全部命中")
        return 0
    outside = sorted(values - expected)
    if outside:
        print(f"{label}出现白名单之外的值", file=sys.stderr)
        for item in outside:
            print(f"  越界：{item}", file=sys.stderr)
        return 1
    if args.verbose:
        print(f"rust_source_grep: {label}{len(values)} 项均在白名单内")
    return 0


if __name__ == "__main__":
    sys.exit(main())
