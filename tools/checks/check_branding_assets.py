#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验共享品牌图形 `assets/branding/`：主源 → 自包含 SVG + 各尺寸位图，且四者指纹未变。

图标的主源是艺术位图 `hux.png`（不是矢量），派生规则与命令见同目录 README 与
`tools/generators/gen_branding_icons.py`。本脚本核对：

1. 主源 sha256 == `MASTER_SHA256`（主源被换掉即失败）；
2. `hux.svg` 自包含：`viewBox` == `CANVAS`、`<image>` 覆盖主源整幅、`href` 只允许内嵌 data URI，
   且**内嵌字节的 sha256 == 主源 sha256**（SVG 里贴的必须是这张主源，不是别处拷来的图）；
3. `hux-<边长>.png` 的边长与文件名一致、带 alpha（PNG 颜色类型 6）；
4. 四个文件的聚合 sha256 == `FINGERPRINT`（改了主源或产物就得同步常量）；
5. 有 `rsvg-convert` 时，SVG 能渲染（不同 librsvg 版本的渲染字节可以不同，故只查能否渲染）。

失败信息会把实测值打印出来，便于按实测更新常量。
"""

from __future__ import annotations

import base64
import hashlib
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "assets" / "branding"
MASTER = DIR / "hux.png"
SVG = DIR / "hux.svg"

# 以下三个常量由 `python3 tools/generators/gen_branding_icons.py` 打印，改动图标后按实测更新。
MASTER_SHA256 = "dbb46e2b6700589a9634aef0918630f2f48d3d5effa824d529271c688a605036"
CANVAS = (237, 221, 1116)
FINGERPRINT = "9bc316e2bee17e62514d4c091a1996d85f5d1b3dc9be631d95f96072c550c6df"

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
RGBA_COLOR_TYPE = 6


def fail(message: str) -> None:
    print(f"品牌图形校验失败：{message}", file=sys.stderr)
    raise SystemExit(1)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def png_header(path: Path) -> tuple[int, int, int]:
    """返回 PNG 的 (宽, 高, 颜色类型)；颜色类型 6 = RGBA（图标必须带 alpha）。"""
    head = path.read_bytes()[:26]
    if len(head) < 26 or head[:8] != PNG_SIGNATURE:
        fail(f"{path.name} 不是 PNG")
    width, height = struct.unpack(">II", head[16:24])
    return width, height, head[25]


def fingerprint(paths: list[Path]) -> str:
    """与生成器和 README 里的命令同构：逐文件 `sha256sum` 行再取一次 sha256。"""
    lines = [f"{digest(path)}  {path.name}".encode() for path in paths]
    return hashlib.sha256(b"\n".join(lines) + b"\n").hexdigest()


def check_master() -> tuple[int, int]:
    if not MASTER.is_file():
        fail(f"缺少主源位图 {MASTER.relative_to(ROOT)}")
    width, height, color_type = png_header(MASTER)
    if color_type != RGBA_COLOR_TYPE:
        fail(f"{MASTER.name} 颜色类型 {color_type}，图标主源需要带 alpha（类型 6）")
    actual = digest(MASTER)
    if actual != MASTER_SHA256:
        fail(f"{MASTER.name} 与常量不符：实测 sha256 = {actual}")
    return width, height


def check_svg(master_size: tuple[int, int]) -> None:
    if not SVG.is_file():
        fail(f"缺少自包含 SVG {SVG.relative_to(ROOT)}")
    text = SVG.read_text(encoding="utf-8")
    for marker in ("SPDX-FileCopyrightText", "SPDX-License-Identifier"):
        if marker not in text:
            fail(f"{SVG.name} 缺少 {marker} 头")
    match = re.search(r'viewBox="([^"]+)"', text)
    if match is None:
        fail(f"{SVG.name} 没有 viewBox")
    want_viewbox = f"{CANVAS[0]} {CANVAS[1]} {CANVAS[2]} {CANVAS[2]}"
    if match.group(1) != want_viewbox:
        fail(f"{SVG.name} 的 viewBox 实测 {match.group(1)!r}，与常量 {want_viewbox!r} 不符")

    image_tag = re.search(r"<image\b[^>]*>", text)
    if image_tag is None:
        fail(f"{SVG.name} 没有 <image> 元素")
    tag = image_tag.group(0)
    want_size = f'width="{master_size[0]}" height="{master_size[1]}"'
    if want_size not in tag:
        fail(f"{SVG.name} 的 <image> 未覆盖主源整幅（应含 {want_size}）")

    for href in re.findall(r'href="([^"]*)"', text):
        if not href.startswith("data:image/png;base64,"):
            fail(f"{SVG.name} 含外部引用（应自包含）：{href[:60]}")
    payload = re.search(r"base64,([A-Za-z0-9+/=]+)", text)
    if payload is None:
        fail(f"{SVG.name} 没有内嵌 base64 位图")
    embedded = base64.b64decode(payload.group(1))
    actual = hashlib.sha256(embedded).hexdigest()
    if actual != MASTER_SHA256:
        fail(f"{SVG.name} 内嵌的不是当前主源：内嵌 sha256 = {actual}")


def check_bitmaps() -> list[Path]:
    bitmaps = sorted(DIR.glob("hux-*.png"))
    if not bitmaps:
        fail(f"{DIR.relative_to(ROOT)} 下没有 hux-<边长>.png")
    for bitmap in bitmaps:
        suffix = bitmap.stem.removeprefix("hux-")
        if not suffix.isdigit():
            fail(f"{bitmap.name} 命名应为 hux-<边长>.png")
        want = int(suffix)
        width, height, color_type = png_header(bitmap)
        if (width, height) != (want, want):
            fail(f"{bitmap.name} 实际 {width}x{height}，与文件名期望 {want}x{want} 不一致")
        if color_type != RGBA_COLOR_TYPE:
            fail(f"{bitmap.name} 颜色类型 {color_type}，图标需要带 alpha（类型 6）")
    return bitmaps


def main() -> int:
    master_size = check_master()
    check_svg(master_size)
    bitmaps = check_bitmaps()

    files = [MASTER, SVG, *bitmaps]
    actual = fingerprint(files)
    if actual != FINGERPRINT:
        fail("四个文件的聚合 sha256 与常量不符：实测 = " + actual)

    renderer = shutil.which("rsvg-convert")
    if renderer is None:
        print(f"品牌图形校验通过（主源 {master_size[0]}x{master_size[1]} 内嵌于 SVG；{len(bitmaps)} 个位图 + 指纹一致；未装 rsvg-convert，跳过渲染检查）")
        return 0
    # 渲染结果走 stdout 丢弃：`-o` 指向非普通文件会被 librsvg 拒绝，而临时文件在只读沙箱里可能不可用。
    # 输出是 PNG 字节，故不按文本解码（只在失败时解码 stderr）。
    result = subprocess.run([renderer, "-w", "16", "-h", "16", str(SVG)], capture_output=True)
    if result.returncode != 0:
        reason = result.stderr.decode("utf-8", "replace").strip()
        fail(f"{SVG.name} 无法渲染：{reason}")
    print(
        f"品牌图形校验通过（主源 {master_size[0]}x{master_size[1]} 内嵌于 SVG、可渲染；"
        f"{len(bitmaps)} 个位图 + 指纹一致）"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
