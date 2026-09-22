#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

"""从 librime `src/rime/key_table.cc` 生成 Rust 键名表。

    python3 tools/generators/gen_key_table.py --source <librime>/src/rime/key_table.cc \
        --out crates/hux-core/src/key_table.rs [--keyvals-out <path>]

--keyvals-out 输出全部键值（每行一个十进制数），供键金样探针使用。
"""
import argparse
import hashlib
import os
import re
import sys
import tempfile
from pathlib import Path


def unescape(segment: str) -> bytes:
    out = bytearray()
    i = 0
    while i < len(segment):
        ch = segment[i]
        if ch != "\\":
            out.extend(ch.encode("utf-8"))
            i += 1
            continue
        i += 1
        if i >= len(segment):
            raise ValueError("dangling escape")
        esc = segment[i]
        if esc == "0":
            out.append(0)
        elif esc == "n":
            out.append(0x0A)
        elif esc == "t":
            out.append(0x09)
        elif esc in "\\'\"":
            out.extend(esc.encode("ascii"))
        elif esc == "x":
            out.append(int(segment[i + 1 : i + 3], 16))
            i += 2
        else:
            raise ValueError(f"unsupported escape \\{esc}")
        i += 1
    return bytes(out)


def extract_names(source: str) -> bytes:
    match = re.search(r"static const char key_names\[\] =(.*?);", source, re.S)
    if not match:
        raise ValueError("key_names literal not found")
    literal = match.group(1)
    segments = re.findall(r'"(?:[^"\\]|\\.)*"', literal)
    return b"".join(unescape(segment[1:-1]) for segment in segments)


def extract_entries(source: str, marker: str) -> list[tuple[int, int]]:
    match = re.search(re.escape(marker) + r".*?\{(.*?)\};", source, re.S)
    if not match:
        raise ValueError(f"{marker} not found")
    body = match.group(1)
    return [
        (int(keyval, 0), int(offset))
        for keyval, offset in re.findall(
            r"\{\s*(0x[0-9a-fA-F]+|\d+)\s*,\s*(\d+)\s*\}", body
        )
    ]


def extract_modifier_names(source: str) -> list[str | None]:
    match = re.search(r"static const char\* modifier_name\[\] = \{(.*?)\};", source, re.S)
    if not match:
        raise ValueError("modifier_name not found")
    values: list[str | None] = []
    for token in re.findall(r'"(?:[^"\\]|\\.)*"|NULL', match.group(1)):
        if token == "NULL":
            values.append(None)
        else:
            values.append(token[1:-1])
    return values


def name_at(names: bytes, offset: int) -> str:
    tail = names[offset:]
    end = tail.find(b"\0")
    raw = tail if end < 0 else tail[:end]
    return raw.decode("utf-8")


def rust_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def write_atomic(path: Path, text: str) -> None:
    """同目录临时文件 + `os.replace`：中断/磁盘满不会留下截断的目标文件。

    `mkstemp` 建的是 0600，替换前先对齐目标原有权限（新文件用 0644）。
    目标是非常规文件（`--out /dev/null` 这类丢弃产物的调用）时退回直接写：
    那种目标没有「截断」可言，且其所在目录通常不可写。
    """
    if path.exists() and not path.is_file():
        path.write_text(text, encoding="utf-8")
        return
    mode = path.stat().st_mode & 0o777 if path.exists() else 0o644
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=path.name + ".", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--keyvals-out")
    parser.add_argument("--provenance", default="librime")
    args = parser.parse_args()

    source = Path(args.source).read_text(encoding="utf-8")
    digest = hashlib.sha256(source.encode("utf-8")).hexdigest()
    names = extract_names(source)
    by_keyval = extract_entries(source, "static const key_entry keys_by_keyval[]")
    by_name = extract_entries(source, "static const key_entry keys_by_name[]")
    modifiers = extract_modifier_names(source)
    if len(modifiers) != 32:
        raise ValueError(f"expected 32 modifier slots, got {len(modifiers)}")

    lines: list[str] = []
    # REUSE-IgnoreStart
    lines.append("// SPDX-FileCopyrightText: 2014 RIME Developers")
    lines.append("// SPDX-License-Identifier: BSD-3-Clause")
    # REUSE-IgnoreEnd
    lines.append("//")
    lines.append("//! 由 librime `src/rime/key_table.cc` 生成（请勿手改）。")
    lines.append("//!")
    lines.append("//! 生成：`python3 tools/generators/gen_key_table.py --source <librime>/src/rime/key_table.cc --out <此文件>`")
    lines.append(f"//! 来源：{args.provenance}")
    lines.append(f"//! key_table.cc sha256：`{digest}`")
    lines.append("")
    lines.append("/// 修饰位名（索引 = 位号；缺失位为 None）。")
    lines.append("pub static MODIFIER_NAMES: [Option<&str>; 32] = [")
    for value in modifiers:
        lines.append(f"    {('None' if value is None else 'Some(' + rust_string(value) + ')')},")
    lines.append("];")
    lines.append("")
    lines.append("/// 按名字查找键值时的扫描序（与 librime 同序，首个匹配生效）。")
    lines.append(f"pub static KEYS_BY_KEYVAL: [(i32, &str); {len(by_keyval)}] = [")
    for keyval, offset in by_keyval:
        lines.append(f"    (0x{keyval:06x}, {rust_string(name_at(names, offset))}),")
    lines.append("];")
    lines.append("")
    lines.append("/// 按键值查找键名时的扫描序（与 librime 同序，首个匹配生效）。")
    lines.append(f"pub static KEYS_BY_NAME: [(i32, &str); {len(by_name)}] = [")
    for keyval, offset in by_name:
        lines.append(f"    (0x{keyval:06x}, {rust_string(name_at(names, offset))}),")
    lines.append("];")
    lines.append("")

    write_atomic(Path(args.out), "\n".join(lines))
    if args.keyvals_out:
        keyvals = sorted({keyval for keyval, _ in by_keyval} | {keyval for keyval, _ in by_name})
        write_atomic(
            Path(args.keyvals_out),
            "\n".join(str(keyval) for keyval in keyvals) + "\n",
        )
    print(
        f'{{"by_keyval":{len(by_keyval)},"by_name":{len(by_name)},'
        f'"modifiers":32,"source_sha256":"{digest}"}}'
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ValueError as error:
        # 已知的输入/解析错误：干净退出（`gen_key_table: <消息>`，退出码 1），不打印 traceback。
        sys.exit(f"gen_key_table: {error}")
