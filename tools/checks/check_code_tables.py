#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验码表（主表 + 追加表）：行格式、跨表去重、「只补主表没有的字」与命名口径一致。

对象：`data/tiger_sentence.codes.txt`（主表）与同目录的 `tiger_sentence.codes.<name>.txt`
（追加表；内核按文件名字典序把它们拼在主表之后，见 `data/README.md` 的「追加码表」）。

核对：
1. **命名口径与内核一致**：`tiger_sentence.codes.` 前缀 + `.txt` 后缀 + `<name>` 非空；
   目录与非法 UTF-8 文件名都跳过（内核 `into_string().ok()` 与读取失败即跳过）；
2. **行格式（仓库口径，比内核更严）**：恰好两列、以制表符分隔、`code` 纯小写 a–z、
   `text` 非空、无 BOM、无 CR；注释以（去空白后的）`#` 开头——与内核的跳过规则一致，
   免得守卫算进去的行内核其实当注释扔掉；
3. `(text, code)` 在所有表里唯一（内核「保首见」地静默去重，重复属登记错误）；
4. **追加表只补主表没有的字**：追加表任一行的文本若已在主表里，说明生成器把主表已有字的
   官方码也并了进来——那会改该字的 `optimal_single`（"整串直出"奖励不再可达），
   不是「只追加」。这条由 `crates/hux-scheme/tiger/tests/shipped_data.rs` 从装载侧再钉一遍；
5. 打印各表条目 / 字数 / 码数，便于人工核对规模。

只读；CI 与本地都能跑。
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"
PRIMARY = DATA / "tiger_sentence.codes.txt"
EXTRA_PREFIX = "tiger_sentence.codes."
EXTRA_SUFFIX = ".txt"
BOM = "\ufeff"


def fail(message: str) -> None:
    print(f"check_code_tables: {message}", file=sys.stderr)
    raise SystemExit(1)


def has_surrogate(name: str) -> bool:
    """非法 UTF-8 文件名在 Python 里以代理码位出现；内核 `into_string().ok()` 会丢弃它们。"""
    return any(0xD800 <= ord(char) <= 0xDFFF for char in name)


def extra_tables() -> list[Path]:
    """与内核同一命名规则（前缀 + 后缀 + `<name>` 非空），跳过目录与非法 UTF-8 名，字典序。"""
    tables = []
    for path in sorted(DATA.glob(f"{EXTRA_PREFIX}*{EXTRA_SUFFIX}")):
        name = path.name
        if has_surrogate(name) or not path.is_file():
            continue
        if name[len(EXTRA_PREFIX) : -len(EXTRA_SUFFIX)]:
            tables.append(path)
    return tables


def parse(path: Path) -> list[tuple[str, str]]:
    raw = path.read_bytes()
    if raw.startswith(b"\xef\xbb\xbf"):
        fail(f"{path.name} 带 BOM：仓库里的码表保持无 BOM（内核会逐表剥，但没必要带）")
    text = raw.decode("utf-8")
    if "\r" in text:
        fail(f"{path.name} 含 CR：仓库里的码表用 LF")
    rows: list[tuple[str, str]] = []
    for number, line in enumerate(text.split("\n"), start=1):
        stripped = line.strip(" \t\u000b\u000c\r")
        if not stripped or stripped.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 2:
            fail(f"{path.name}:{number} 不是 `<text>\\t<code>` 两列：{line!r}")
        word, code = parts
        if not word:
            fail(f"{path.name}:{number} 文本为空：{line!r}")
        if not code or not code.isascii() or not code.islower() or not code.isalpha():
            fail(f"{path.name}:{number} 码不是纯小写 a–z：{line!r}")
        rows.append((word, code))
    return rows


def main() -> int:
    if not PRIMARY.is_file():
        fail(f"缺少主表 {PRIMARY.relative_to(ROOT)}")
    tables = [PRIMARY, *extra_tables()]

    seen: dict[tuple[str, str], str] = {}
    duplicates: list[str] = []
    primary_words: set[str] = set()
    extra_known_words: list[str] = []
    total = 0
    print(f"check_code_tables: {len(tables)} 张表（主表 + {len(tables) - 1} 张追加表）")
    for index, path in enumerate(tables):
        rows = parse(path)
        words = {word for word, _ in rows}
        codes = {code for _, code in rows}
        if index == 0:
            primary_words = words
        else:
            extra_known_words.extend(sorted(words & primary_words))
        for word, code in rows:
            key = (word, code)
            if key in seen:
                duplicates.append(f"{word}\t{code}（{seen[key]} 与 {path.name} 重复）")
            else:
                seen[key] = path.name
        total += len(rows)
        print(f"  {path.name}：{len(rows)} 条、{len(words)} 个文本、{len(codes)} 个码")

    if duplicates:
        print("以下 (字, 码) 在多张表里重复（追加表只应放主表没有的对）：", file=sys.stderr)
        for item in duplicates[:10]:
            print(f"  · {item}", file=sys.stderr)
        if len(duplicates) > 10:
            print(f"  … 共 {len(duplicates)} 条", file=sys.stderr)
        raise SystemExit(1)

    if extra_known_words:
        print(
            f"追加表里有 {len(extra_known_words)} 个文本已在主表（只允许补主表没有的字）：",
            file=sys.stderr,
        )
        for word in extra_known_words[:10]:
            print(f"  · {word}", file=sys.stderr)
        if len(extra_known_words) > 10:
            print(f"  … 共 {len(extra_known_words)} 个", file=sys.stderr)
        raise SystemExit(1)

    print(f"  ✓ 合计 {total} 条、{len({word for word, _ in seen})} 个文本、跨表无重复")
    print(f"  ✓ 追加表只补主表没有的字（主表 {len(primary_words)} 个文本）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
