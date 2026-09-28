#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""守卫脚本与生成器共享的哈希助手。

各调用方的**读取口径不同**（整读 / 分块流式 / 文本编码 / 内存字节），故口径各自成函数、
不合并语义；共享的是「同一字节序 ⇒ 同一摘要」的计算本身。聚合指纹同此：
逐文件摘要拼成 `<sha256>  <标签>` 行后再取一次 sha256（与 `sha256sum | sha256sum` 同构）。

`tools/generators/` 下的生成器跨目录使用本模块时，需先把 `tools/checks/` 加入 `sys.path`。
"""

from __future__ import annotations

import hashlib
from collections.abc import Iterable
from pathlib import Path

# 流式读取的块大小：1 MiB（金样、拼音索引等大文件，避免整读进内存）。
CHUNK_SIZE = 1 << 20


def sha256_bytes(payload: bytes) -> str:
    """已在内存里的字节串的 sha256。"""
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: Path) -> str:
    """整读文件的 sha256（小文件；大文件用 [`sha256_stream`]，两者摘要相同）。"""
    return sha256_bytes(Path(path).read_bytes())


def sha256_stream(path: Path, chunk_size: int = CHUNK_SIZE) -> str:
    """按块流式读取文件的 sha256（读法与整读不同，摘要同源）。"""
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(chunk_size), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_text(text: str) -> str:
    """文本按 UTF-8 编码后的 sha256（文本口径：编码与换行都进摘要）。"""
    return sha256_bytes(text.encode("utf-8"))


def fingerprint(entries: Iterable[tuple[str, str]]) -> str:
    """聚合指纹：每条 `(sha256, 标签)` 拼成一行 `<sha256>  <标签>`（行尾换行）后整体再取 sha256。

    标签由调用方给（文件名或相对路径）：它进入摘要，是「聚合了哪些文件」的证据。
    调用方保证 `entries` 非空——空集合在各调用点都另有失败分支。
    """
    lines = "".join(f"{digest}  {label}\n" for digest, label in entries)
    return sha256_bytes(lines.encode("utf-8"))
