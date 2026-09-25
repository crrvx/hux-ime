#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""生成 `assets/branding/` 的图标产物：主源 `hux.png` → 自包含 `hux.svg` + 各尺寸位图。

主源是艺术位图（RGBA），本脚本只做**版式归一**，不重绘、不压缩画质：

1. 取不透明内容的外接矩形（alpha 高于 `ALPHA_THRESHOLD`，忽略几乎不可见的辉光尾）；
2. 画布 = 内容外接**正方形** + 四周 8% 留白，内容居中（画布越出主源时就近贴边）；
3. `hux.svg`：内嵌主源字节（base64 data URI，与主源逐字节相同），`viewBox` 即该画布——自包含、
   无外部引用，缩小渲染到任意尺寸都清晰（只有放大超过主源分辨率才会软化）；
4. 各尺寸位图：同一画布用 Lanczos 缩小。

只在改动图标时手动跑（CI 只跑 `tools/checks/check_branding_assets.py` 做结构校验）：

    python3 tools/generators/gen_branding_icons.py

跑完把 `assets/branding/` 下的文件一起提交，并按脚本末尾打印的值更新守卫里的
`MASTER_SHA256` / `CANVAS` / `FINGERPRINT` 三个常量。
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "assets" / "branding"
MASTER = DIR / "hux.png"
SVG = DIR / "hux.svg"
SIZES = (22, 48)
PADDING_RATIO = 0.08
# 取内容外接框时忽略的 alpha 上限（0–255）：辉光尾部的 alpha 只有个位数，肉眼不可见，
# 但它会把外接框撑到接近整幅，导致小尺寸下图形式微、四周留白不均。
ALPHA_THRESHOLD = 8

HEADER = """<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- 虎虚（hux）图标：由 tools/generators/gen_branding_icons.py 生成，勿手改；
     主源 assets/branding/hux.png（艺术位图）在下方以 data URI 内嵌，与主源逐字节相同。 -->
"""


def canvas_box(width: int, height: int, bbox: tuple[int, int, int, int]) -> tuple[int, int, int]:
    """内容外接矩形 → 正方形画布（左上角 + 边长）：内容居中，四周留 PADDING_RATIO 留白。"""
    left, top, right, bottom = bbox
    content = max(right - left, bottom - top)
    side = content + 2 * round(content * PADDING_RATIO)
    side = min(side, min(width, height))
    x = round((left + right) / 2 - side / 2)
    y = round((top + bottom) / 2 - side / 2)
    # 画布必须完整落在主源内（越界就近贴边，保持内容可见）。
    x = min(max(x, 0), width - side)
    y = min(max(y, 0), height - side)
    return x, y, side


def main() -> int:
    parser = argparse.ArgumentParser(description="生成 assets/branding 的图标产物")
    parser.add_argument("--master", default=str(MASTER), help="主源位图（缺省 assets/branding/hux.png）")
    args = parser.parse_args()

    try:
        from PIL import Image
    except ImportError:
        print("生成失败：需要 Pillow（pip install pillow）", file=sys.stderr)
        return 1

    master = Path(args.master)
    if not master.is_file():
        print(f"生成失败：缺少主源 {master}", file=sys.stderr)
        return 1
    payload = master.read_bytes()
    image = Image.open(master).convert("RGBA")
    master_size = (image.width, image.height)
    visible = image.getchannel("A").point(lambda value: 255 if value > ALPHA_THRESHOLD else 0)
    bbox = visible.getbbox()
    if bbox is None:
        print(f"生成失败：主源在 alpha > {ALPHA_THRESHOLD} 上没有可见内容", file=sys.stderr)
        return 1
    x, y, side = canvas_box(image.width, image.height, bbox)
    image = image.crop((x, y, x + side, y + side))

    encoded = base64.b64encode(payload).decode("ascii")
    SVG.write_text(
        HEADER
        + '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"\n'
        + f'     viewBox="{x} {y} {side} {side}" width="{side}" height="{side}">\n'
        + f'  <image x="0" y="0" width="{master_size[0]}" height="{master_size[1]}"'
        + f' xlink:href="data:image/png;base64,{encoded}"/>\n'
        + "</svg>\n",
        encoding="utf-8",
    )

    sizes = []
    for size in SIZES:
        target = DIR / f"hux-{size}.png"
        resized = image.resize((size, size), Image.LANCZOS)
        resized.save(target, format="PNG", optimize=True)
        sizes.append(target)

    print(
        f"主源 {master.name}：{master_size[0]}x{master_size[1]} RGBA，"
        f"内容外接 {bbox[2] - bbox[0]}x{bbox[3] - bbox[1]}"
    )
    print(f"画布（viewBox）：{x} {y} {side} {side}")
    print(f"已写出 {SVG.name}（内嵌 {len(payload)} 字节）+ " + "、".join(t.name for t in sizes))
    print()
    print("守卫常量：")
    print(f'MASTER_SHA256 = "{hashlib.sha256(payload).hexdigest()}"')
    print(f"CANVAS = ({x}, {y}, {side})")
    print(f'FINGERPRINT = "{fingerprint([master, SVG, *sizes])}"')
    return 0


def fingerprint(paths: list[Path]) -> str:
    """四个文件的聚合 sha256：逐文件 `sha256sum` 行（同 README 里的命令）再取一次 sha256。"""
    lines = []
    for path in paths:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {path.name}".encode())
    return hashlib.sha256(b"\n".join(lines) + b"\n").hexdigest()


if __name__ == "__main__":
    import sys

    raise SystemExit(main())
