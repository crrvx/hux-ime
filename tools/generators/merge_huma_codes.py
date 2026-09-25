#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""生成追加码表 `data/tiger_sentence.codes.huma.txt`：虎码官方版单字表里**主表没有的字**的 `(字, 码)`。

动机：主表（`data/tiger_sentence.codes.txt`，取自虎整句上游）只收常用字，生僻字打不出来。
内核把 `tiger_sentence.codes.<name>.txt` 拼在主表之后（见 `data/README.md` 的「追加码表」），
因此主表内所有 rank 逐位不变（简码仍归主表），本表只能在既有码上垫后或引入新码；
**删掉本文件即完全回滚**。

只收主表没有的字：给主表已有的字补官方短码会改它的最优码（`optimal_single` 由 true 变 false，
「整串直出」奖励不再可达），那就不叫「只追加」了——主表已有字的拼写一律以虎句主表为准。

源不入库，按路径给出（官方版解包后的 `publish/rime/tiger.dict.yaml`）：

    HUMA_DICT=~/下载/zhhmn/publish/rime/tiger.dict.yaml \
        python3 tools/generators/merge_huma_codes.py

输出按「码 → 官方权重降序 → 字」排序：同一码内先出高频字。同源必得同字节（源里带 version、
生成时把源的 sha256 写进表头，故表头也不随机器变化）；改了源就重跑并更新 `data/MANIFEST` 与
`goldens/regenerate.md` 里的 sha。
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PRIMARY = ROOT / "data" / "tiger_sentence.codes.txt"
TARGET = ROOT / "data" / "tiger_sentence.codes.huma.txt"
YAML_HEADER = re.compile(r"^[a-z_]+:")


def fail(message: str) -> None:
    print(f"merge_huma_codes: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_primary(path: Path) -> set[tuple[str, str]]:
    """主表已有的 `(字/词, 码)` 对（用于去重；主表本身不会被改写）。"""
    if not path.is_file():
        fail(f"缺少主表 {path}")
    pairs: set[tuple[str, str]] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 2:
            pairs.add((parts[0], parts[1]))
    return pairs


def read_source(path: Path) -> tuple[str, list[tuple[str, str, int]]]:
    """官方版单字表 → (version, [(字, 码, 权重)])；只收单字 + a–z 码。"""
    version = "未知"
    rows: list[tuple[str, str, int]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("version:"):
            version = line.split(":", 1)[1].strip().strip('"')
            continue
        if not line or line.lstrip().startswith("#") or line.startswith(("---", "...", "  - ")):
            continue
        if YAML_HEADER.match(line):
            continue
        parts = re.split(r"\t+", line)
        if len(parts) < 2:
            continue
        text, code = parts[0], parts[1]
        if len(text) != 1 or not re.fullmatch(r"[a-z]+", code):
            continue
        weight = int(parts[2]) if len(parts) > 2 and parts[2].isdigit() else 0
        rows.append((text, code, weight))
    if not rows:
        fail(f"源里没有可用的单字行：{path}")
    return version, rows


def main() -> int:
    parser = argparse.ArgumentParser(description="生成虎码官方版全字集的追加码表")
    parser.add_argument(
        "--source",
        default=os.environ.get("HUMA_DICT", ""),
        help="虎码官方版 tiger.dict.yaml（也可用环境变量 HUMA_DICT）",
    )
    parser.add_argument("--out", default=str(TARGET), help="输出路径（缺省 data/tiger_sentence.codes.huma.txt）")
    args = parser.parse_args()

    if not args.source:
        fail("请给出官方单字表路径：--source <tiger.dict.yaml> 或 HUMA_DICT=<路径>")
    source = Path(args.source).expanduser()
    if not source.is_file():
        fail(f"找不到源文件 {source}")

    primary = read_primary(PRIMARY)
    primary_chars = {text for text, _ in primary if len(text) == 1}
    version, rows = read_source(source)
    payload = source.read_bytes()
    digest = hashlib.sha256(payload).hexdigest()

    # 只收**主表没有的字**：给主表已有的字补官方短码会改它的最优码
    # （`optimal_single` 由 true 变 false，"整串直出"奖励不再可达），
    # 那就不是「只追加」了。主表已有字的拼写一律以主表（虎句）为准。
    added = [
        (text, code, weight)
        for text, code, weight in rows
        if text not in primary_chars and (text, code) not in primary
    ]
    added.sort(key=lambda row: (row[1], -row[2], row[0]))

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    # REUSE-IgnoreStart
    # （下面这段是**写进生成物**的许可头；行内的标识不该被当成这个脚本自己的 SPDX 声明——
    #   `LicenseRef-…` 带引号时 reuse 解析不了，故按规范用 Ignore 区隔。）
    header = [
        "# SPDX-FileCopyrightText: 虎码官方（虎码输入法官方版）作者与贡献者；2026 明雅流风 <crrvx@outlook.com>",
        "# SPDX-License-Identifier: LicenseRef-HuMa-Official",
        "#",
        "# 追加码表：虎码官方版单字表里主表没有的 (字, 码)。格式与主表一致：每行 \"<text>\\t<code>\"，",
        "# 同码内的行序即 rank。内核把本表拼在主表之后 ⇒ 主表 rank 逐位不变（简码仍归主表），",
        "# 本表只在既有码上垫后或引入新码；删除本文件即完全回滚。",
        f"#",
        f"# 源：虎码官方版 publish/rime/tiger.dict.yaml（version {version}，sha256 {digest}）",
        "# 生成：HUMA_DICT=<官方单字表路径> python3 tools/generators/merge_huma_codes.py",
        f"# 本表 {len(added)} 行；同码内按官方权重降序；授权口径见 LICENSES/LicenseRef-HuMa-Official.txt。",
        "",
    ]
    # REUSE-IgnoreEnd
    body = [f"{text}\t{code}" for text, code, _ in added]
    out.write_text("\n".join(header + body) + "\n", encoding="utf-8")

    chars = {text for text, _, _ in added}
    codes = {code for _, code, _ in added}
    primary_chars = {text for text, _ in primary if len(text) == 1}
    new_chars = {text for text in chars if text not in primary_chars}
    print(f"源 {source.name}：version {version}，sha256 {digest[:12]}…，单字行 {len(rows)}")
    print(f"主表已有 {len(primary)} 对（{len(primary_chars)} 个字）；本表追加 {len(added)} 对：")
    print(f"  涉及 {len(chars)} 个字（其中主表没有的新字 {len(new_chars)}）、{len(codes)} 个码")
    print(f"已写出 {out.relative_to(ROOT)}（{out.stat().st_size} 字节）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
