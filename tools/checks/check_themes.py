#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验共享主题 `assets/themes/`：清单 ↔ 目录一致、每套文件齐备、引用图存在、取用指纹未变。

指纹是「全部主题文件的聚合 sha256」，钉住取用时的内容（与 CI 里四份不重生成的金样同款做法）：
新增或更新主题时重算并更新本文件的常量，命令：

    cd assets/themes && find hufu-* -type f | sort | xargs sha256sum | sha256sum
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "assets" / "themes"
MANIFEST = DIR / "MANIFEST"
EXPECTED_DIGEST = "7ad673c4c6df5330db8fc84566a65b93ab39c6686de12428f7caa208208c7a9d"
EXPECTED_FILES = (
    "theme.conf",
    "panel.png",
    "highlight.png",
    "prev.png",
    "next.png",
    "arrow.png",
    "radio.png",
)


def fail(message: str) -> None:
    print(f"主题校验失败：{message}", file=sys.stderr)
    raise SystemExit(1)


def manifest_entries() -> list[str]:
    if not MANIFEST.is_file():
        fail("缺少 assets/themes/MANIFEST")
    entries = []
    for raw in MANIFEST.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        entries.append(line)
    if not entries:
        fail("MANIFEST 没有有效行")
    return entries


def main() -> int:
    entries = manifest_entries()
    for name in entries:
        theme = DIR / name
        if not theme.is_dir():
            fail(f"MANIFEST 列出的主题目录不存在：{name}")
        for expected in EXPECTED_FILES:
            if not (theme / expected).is_file():
                fail(f"{name} 缺少 {expected}")
        # `theme.conf` 里 `Image=` 引用的图必须同目录存在。
        for raw in (theme / "theme.conf").read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if not line.startswith("Image="):
                continue
            image = line.removeprefix("Image=").strip()
            if not (theme / image).is_file():
                fail(f"{name}/theme.conf 引用的图不存在：{image}")
    # 目录与清单互为全集（多出来的主题目录说明漏登记）。
    on_disk = sorted(p.name for p in DIR.iterdir() if p.is_dir())
    if on_disk != sorted(entries):
        fail(f"目录与 MANIFEST 不一致：目录 {on_disk} vs 清单 {sorted(entries)}")
    # 聚合指纹：只看清单里的主题文件（本目录的 README/MANIFEST 不算），
    # 逐文件 sha256 按相对路径排序后拼接（与 `sha256sum` 的输出格式一致）。
    files = sorted(
        path
        for name in entries
        for path in (DIR / name).rglob("*")
        if path.is_file()
    )
    lines = []
    for path in files:
        relative = path.relative_to(DIR).as_posix()
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {relative}\n")
    aggregate = hashlib.sha256("".join(lines).encode("utf-8")).hexdigest()
    if aggregate != EXPECTED_DIGEST:
        fail(
            "取用指纹与常量不一致：\n"
            f"  实际 {aggregate}\n  期望 {EXPECTED_DIGEST}\n"
            "（重算：cd assets/themes && find hufu-* -type f | sort | xargs sha256sum | sha256sum）"
        )
    print(f"主题校验通过（{len(entries)} 套 / {len(lines)} 个文件，指纹一致）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
