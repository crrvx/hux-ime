#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验共享品牌图形 `assets/branding/`：矢量源存在、位图尺寸与文件名一致、可渲染。

位图与矢量的一致性靠「位图由 `hux.svg` 生成」这一约定（生成命令见同目录 README）；
本脚本不逐字节比对——不同 librsvg 版本的渲染字节可能不同，逐字节会把版本差异误报成回归。
"""

from __future__ import annotations

import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "assets" / "branding"
SVG = DIR / "hux.svg"


def fail(message: str) -> None:
    print(f"品牌图形校验失败：{message}", file=sys.stderr)
    raise SystemExit(1)


def png_size(path: Path) -> tuple[int, int]:
    with path.open("rb") as handle:
        head = handle.read(24)
    if len(head) < 24 or head[:8] != b"\x89PNG\r\n\x1a\n":
        fail(f"{path.name} 不是 PNG")
    return struct.unpack(">II", head[16:24])


def main() -> int:
    if not SVG.is_file():
        fail(f"缺少矢量源 {SVG.relative_to(ROOT)}")
    bitmaps = sorted(DIR.glob("hux-*.png"))
    if not bitmaps:
        fail(f"{DIR.relative_to(ROOT)} 下没有 hux-<边长>.png")
    for bitmap in bitmaps:
        suffix = bitmap.stem.removeprefix("hux-")
        if not suffix.isdigit():
            fail(f"{bitmap.name} 命名应为 hux-<边长>.png")
        want = int(suffix)
        width, height = png_size(bitmap)
        if (width, height) != (want, want):
            fail(f"{bitmap.name} 实际 {width}x{height}，与文件名期望 {want}x{want} 不一致")
    renderer = shutil.which("rsvg-convert")
    if renderer is None:
        print(f"品牌图形校验通过（{len(bitmaps)} 个位图；未装 rsvg-convert，跳过 SVG 渲染检查）")
        return 0
    # 渲染结果走 stdout 丢弃：`-o` 指向非普通文件会被 librsvg 拒绝，而临时文件在只读沙箱里可能不可用。
    # 输出是 PNG 字节，故不按文本解码（只在失败时解码 stderr）。
    result = subprocess.run(
        [renderer, "-w", "16", "-h", "16", str(SVG)],
        capture_output=True,
    )
    if result.returncode != 0:
        reason = result.stderr.decode("utf-8", "replace").strip()
        fail(f"{SVG.name} 无法渲染：{reason}")
    print(f"品牌图形校验通过（{len(bitmaps)} 个位图 + SVG 可渲染）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
