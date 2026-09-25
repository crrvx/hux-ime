#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验资源总账 `docs/resources.md`：随仓资源是否都登记、许可是否与 `REUSE.toml` 一致。

三件事，任一不符即 `exit 1`：

1. **登记（无遗漏）**：下列来源里的每个资源路径都必须在 `docs/resources.md` 的**表格**中被登记，
   或被表里登记的 glob 覆盖——
   `data/MANIFEST` 的随包数据、`assets/themes/MANIFEST` 的主题目录、`assets/branding/`、
   `goldens/**`（含子目录）、`crates/hux-core/src/key_table.rs`、`docs/images/`、
   `LICENSES/` 与根 `LICENSE`、`REUSE.toml`、`platform/fcitx5/conf/*.conf`。
   **随包 / 逐条登记类**（数据、主题、品牌图形、插件 conf、文档图片、许可证文本）要求被**等值登记**
   （精确路径或目录前缀，宽 glob 不算）——新增一个随包资源却不登记即失败；
   `goldens/**` 属成组登记（金样按组说明即可），可用 glob 覆盖，但整组一行都不能少。
2. **许可一致（可溯源）**：`REUSE.toml` 注解覆盖的路径，其 SPDX 许可必须与账本里该资源所在行写的
   许可**相同**（行按「最具体优先」选定：精确路径 > 目录前缀 > glob，字面前缀长者优先）。
   `REUSE.toml` 写 `GPL-3.0-only` 而账本写别的许可即失败。
3. **必备声明**：无法从文件系统枚举的条目（服务与端口、模型获取渠道与格式、运行时可写数据文件）
   按 `REQUIRED_MARKERS` 的关键字核对。

用法：
    python3 tools/checks/check_resources.py              # 以仓库根为根
    python3 tools/checks/check_resources.py --root DIR   # 指定根（副本反向验证用）
"""

from __future__ import annotations

import argparse
import fnmatch
import re
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    tomllib = None  # type: ignore[assignment]

LEDGER = Path("docs/resources.md")
REUSE = Path("REUSE.toml")
# 清单与说明文档不是资源本体，不要求逐条登记。
SKIP_NAMES = frozenset({"MANIFEST", "README.md"})
# 账本「许可」列认得的 SPDX 标识；REUSE.toml 里出现的标识会自动并入。
KNOWN_SPDX = frozenset(
    {
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "CC-BY-4.0",
        "CC-BY-SA-4.0",
        "BSD-3-Clause",
        "Apache-2.0",
        "MIT",
    }
)
# 无法从文件系统枚举的必备声明（关键字 → 缺了怎么补）。
REQUIRED_MARKERS: tuple[tuple[str, str], ...] = (
    ("无常驻服务", "在「后台服务与端口」一节写明「无常驻服务」"),
    ("无 socket", "在「后台服务与端口」一节写明「无 socket」"),
    ("无端口", "在「后台服务与端口」一节写明「无端口」"),
    ("libhux.so", "在「落点与查找顺序」说明插件本体 libhux.so 的落点"),
    ("948170058", "在「模型」一节写明模型的自取渠道（上游 Release / 虎码 QQ 群 948170058）"),
    ("TCSKNM02", "在「模型」一节写明三阶模型格式 TCSKNM02"),
    ("TCSKNM03", "在「模型」一节写明五阶模型格式 TCSKNM03"),
    ("models/sentence-ngram-mobile.bin", "在「模型」一节登记默认模型文件名"),
    ("sentence-fivegram-mobile.bin", "在「模型」一节登记五阶模型文件名"),
    ("tiger_sentence.options.yaml", "在「运行时可写数据」一节登记选项存储"),
    ("tiger_sentence_learning_", "在「运行时可写数据」一节登记学习库"),
    (".userdb", "在「运行时可写数据」一节写明学习库后缀 .userdb"),
    ("conf/hux.conf", "在「运行时可写数据」一节登记 fcitx5 配置文件"),
)


@dataclass(frozen=True)
class Resource:
    """一个随仓存在的资源路径。"""

    path: str
    origin: str
    # True = 随包 / 逐条登记类：必须被等值登记（宽 glob 不算）。
    enumerated: bool


@dataclass(frozen=True)
class Row:
    """账本表格里的一行：登记的路径 / glob + 该行写的许可。"""

    patterns: tuple[str, ...]
    licenses: frozenset[str]
    line: int


@dataclass(frozen=True)
class Annotation:
    """`REUSE.toml` 的一条注解。"""

    pattern: str
    license: str
    override: bool


def fail(message: str) -> None:
    print(f"FAIL {message}", file=sys.stderr)


def read_entries(path: Path) -> list[str]:
    """读清单类文件（每行一条，`#` 与空行是注释）。"""
    return [
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]


def files_under(directory: Path, *, recursive: bool = False, suffixes: tuple[str, ...] = ()) -> list[Path]:
    """目录下的普通文件（按路径排序）；目录不存在时为空。"""
    if not directory.is_dir():
        return []
    found = directory.rglob("*") if recursive else directory.iterdir()
    return sorted(
        item
        for item in found
        if item.is_file()
        and item.name not in SKIP_NAMES
        and (not suffixes or item.suffix in suffixes)
    )


def has_glob(pattern: str) -> bool:
    return any(ch in pattern for ch in "*?[")


def literal_prefix_len(pattern: str) -> int:
    return len(re.match(r"[^*?\[]*", pattern).group(0))


def pattern_matches(pattern: str, path: str) -> bool:
    """账本登记的路径 / glob 是否覆盖该资源（目录登记以 `/` 结尾，按前缀匹配）。"""
    bare = pattern.strip().rstrip("/")
    if not bare:
        return False
    if bare == path:
        return True
    if pattern.endswith("/") and path.startswith(bare + "/"):
        return True
    if has_glob(bare):
        return fnmatch.fnmatchcase(path, bare)
    return False


def score(pattern: str, path: str) -> tuple[int, int, int]:
    """匹配具体度：精确 > 前缀/glob 里字面部分更长 > 模式更长。"""
    bare = pattern.rstrip("/")
    return (1 if bare == path else 0, literal_prefix_len(bare), len(bare))


def parse_ledger(text: str, accepted_spdx: frozenset[str]) -> tuple[list[Row], list[str]]:
    """解析账本里的 Markdown 表格：取「路径 / 资源」列与「许可」列。

    返回（登记行，表格结构问题）；单元格数与表头不符会被报出——表格里漏转义的 `|`
    会把后面的列挤走，静默解析会漏判。
    """
    rows: list[Row] = []
    problems: list[str] = []
    header: list[str] | None = None
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line.startswith("|"):
            header = None
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if all(cell and set(cell) <= set("-: ") for cell in cells):
            continue  # 表头分隔行
        if header is None:
            header = cells
            continue
        if len(cells) != len(header):
            problems.append(
                f"{LEDGER} 第 {lineno} 行：单元格 {len(cells)} 个，与表头 {len(header)} 个不符"
                f"（表格内的 `|` 要写成 `\\|`）"
            )
            continue
        path_index = next((i for i, name in enumerate(header) if "路径" in name), None)
        if path_index is None:
            path_index = next((i for i, name in enumerate(header) if "资源" in name), None)
        if path_index is None:
            continue  # 与资源无关的表（如「未随包 / 未使用的第三方」）
        patterns = tuple(
            match.strip() for match in re.findall(r"`([^`]+)`", cells[path_index]) if match.strip()
        )
        if not patterns:
            patterns = tuple(
                part.strip()
                for part in re.split(r"[、,，]|<br\s*/?>", cells[path_index])
                if part.strip()
            )
        license_index = next((i for i, name in enumerate(header) if "许可" in name), None)
        licenses: frozenset[str] = frozenset()
        if license_index is not None:
            licenses = frozenset(
                token
                for token in re.findall(r"[A-Za-z0-9][A-Za-z0-9.+-]*", cells[license_index])
                if token in accepted_spdx
            )
        if patterns:
            rows.append(Row(patterns=patterns, licenses=licenses, line=lineno))
    return rows, problems


def row_for(rows: list[Row], path: str) -> Row | None:
    """最具体优先：精确路径 > 目录前缀 > glob，字面前缀长者优先。"""
    best: Row | None = None
    best_score: tuple[int, int, int] | None = None
    for row in rows:
        for pattern in row.patterns:
            if not pattern_matches(pattern, path):
                continue
            current = score(pattern, path)
            if best_score is None or current > best_score:
                best, best_score = row, current
    return best


def load_reuse(path: Path) -> list[Annotation]:
    """读 `REUSE.toml` 的 `[[annotations]]`（路径 + SPDX 许可 + 是否 override）。"""
    if tomllib is None:
        fail("需要 Python ≥3.11 的 tomllib 来解析 REUSE.toml")
        return []
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    annotations: list[Annotation] = []
    for entry in data.get("annotations", []):
        patterns = entry.get("path", [])
        if isinstance(patterns, str):
            patterns = [patterns]
        license_id = entry.get("SPDX-License-Identifier", "")
        if isinstance(license_id, list):
            license_id = license_id[0] if license_id else ""
        override = entry.get("precedence") == "override"
        annotations.extend(
            Annotation(pattern=pattern, license=license_id, override=override)
            for pattern in patterns
        )
    return annotations


def reuse_license(annotations: list[Annotation], path: str) -> str | None:
    """该路径在 `REUSE.toml` 里的许可（override 优先，其次最具体）。"""
    matches = [item for item in annotations if pattern_matches(item.pattern, path)]
    if not matches:
        return None
    overrides = [item for item in matches if item.override]
    pool = overrides or matches
    best = max(pool, key=lambda item: (literal_prefix_len(item.pattern), len(item.pattern)))
    return best.license


def collect(root: Path) -> list[Resource]:
    """收集随仓存在的资源路径（含来源说明与是否要求逐条登记）。"""
    resources: list[Resource] = []

    def add(relative: str, origin: str, enumerated: bool = True) -> None:
        resources.append(Resource(path=relative, origin=origin, enumerated=enumerated))

    data_manifest = root / "data" / "MANIFEST"
    if data_manifest.is_file():
        for entry in read_entries(data_manifest):
            add(entry, "data/MANIFEST（随包数据）")
    else:
        fail("缺少 data/MANIFEST（随包数据清单）")

    themes_manifest = root / "assets" / "themes" / "MANIFEST"
    if themes_manifest.is_file():
        for name in read_entries(themes_manifest):
            add(f"assets/themes/{name}", "assets/themes/MANIFEST（随包主题）")
    else:
        fail("缺少 assets/themes/MANIFEST（主题清单）")

    for item in files_under(root / "assets" / "branding"):
        add(item.relative_to(root).as_posix(), "assets/branding/（随包品牌图形）")

    for item in files_under(root / "goldens", recursive=True):
        add(item.relative_to(root).as_posix(), "goldens/**（测试金样与夹具）", enumerated=False)

    key_table = root / "crates" / "hux-core" / "src" / "key_table.rs"
    if key_table.is_file():
        add(key_table.relative_to(root).as_posix(), "crates/hux-core/src（源码生成物）")

    for item in files_under(root / "docs" / "images", recursive=True):
        add(item.relative_to(root).as_posix(), "docs/images/（文档图片）")

    for item in files_under(root / "LICENSES", recursive=True):
        add(item.relative_to(root).as_posix(), "LICENSES/（许可正文）")
    if (root / "LICENSE").is_file():
        add("LICENSE", "仓库根（项目许可正文）")
    if (root / REUSE).is_file():
        add(REUSE.as_posix(), "仓库根（许可标注）")

    for item in files_under(root / "platform" / "fcitx5" / "conf", suffixes=(".conf",)):
        add(item.relative_to(root).as_posix(), "platform/fcitx5/conf（随包插件配置）")

    return resources


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        default=None,
        help="仓库根（缺省 = 本脚本上两级目录；副本反向验证时可指定）",
    )
    args = parser.parse_args()
    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parents[2]

    ledger_path = root / LEDGER
    if not ledger_path.is_file():
        fail(f"缺少资源总账 {LEDGER}（本守卫的核对对象）")
        print("check_resources: 0 条登记，1 项失败", file=sys.stderr)
        return 1
    text = ledger_path.read_text(encoding="utf-8")

    reuse_path = root / REUSE
    if reuse_path.is_file():
        reuse_annotations = load_reuse(reuse_path)
    else:
        fail(f"缺少 {REUSE}（许可标注来源）")
        reuse_annotations = []

    accepted_spdx = KNOWN_SPDX | {item.license for item in reuse_annotations if item.license}
    rows, table_problems = parse_ledger(text, accepted_spdx)
    if not rows:
        fail(f"{LEDGER} 里没有解析到任何登记行（表格的路径列需含反引号路径 / glob）")

    resources = collect(root)
    failures = 0
    for problem in table_problems:
        failures += 1
        fail(problem)

    # ① 登记：每个资源都要被账本覆盖；随包 / 逐条登记类必须等值登记。
    for resource in resources:
        matched = [
            row
            for row in rows
            if any(pattern_matches(pattern, resource.path) for pattern in row.patterns)
        ]
        if not matched:
            failures += 1
            how = (
                "随包 / 逐条登记类资源必须逐条列出，宽 glob 不作为登记。"
                if resource.enumerated
                else "成组登记的整组 glob 亦可（如 `goldens/**`），但不能整组缺失。"
            )
            fail(
                f"未登记：{resource.path}（来源：{resource.origin}）\n"
                f"  补法：在 {LEDGER} 的资源表里为它补一行，或把它并入该表已有一行的路径列；\n"
                f"        {how}"
            )
            continue
        if resource.enumerated:
            exact = [
                pattern
                for row in matched
                for pattern in row.patterns
                if pattern_matches(pattern, resource.path) and not has_glob(pattern)
            ]
            if not exact:
                covered = sorted({pattern for row in matched for pattern in row.patterns})
                failures += 1
                fail(
                    f"未逐条登记：{resource.path}（来源：{resource.origin}）\n"
                    f"  它只被 glob 覆盖：{'、'.join(covered)}\n"
                    f"  补法：在 {LEDGER} 里补一条精确路径（或目录前缀，如 `…/`），"
                    f"不要用宽 glob 代指随包资源。"
                )

    # ② 许可：REUSE.toml 注解覆盖的资源，账本写的许可必须与之一致。
    for resource in resources:
        expected = reuse_license(reuse_annotations, resource.path)
        if expected is None:
            continue
        row = row_for(rows, resource.path)
        if row is None:
            continue  # 已在 ① 报过「未登记」
        if row.licenses != {expected}:
            actual = "、".join(sorted(row.licenses)) or "（未写 SPDX 标识）"
            failures += 1
            fail(
                f"许可不一致：{resource.path}\n"
                f"  REUSE.toml 标注：{expected}\n"
                f"  {LEDGER} 第 {row.line} 行（{'、'.join(row.patterns)}）：{actual}\n"
                f"  补法：把该行许可改为 `{expected}`；若该行覆盖多种许可，"
                f"给它另加一行精确路径的单许可登记（行按最具体优先选定）。"
            )

    # ③ 必备声明：无法从文件系统枚举的条目按关键字核对。
    for marker, hint in REQUIRED_MARKERS:
        if marker not in text:
            failures += 1
            fail(f"缺少必备声明：`{marker}`（{hint}）")

    if failures:
        print(f"check_resources: {len(rows)} 条登记，{failures} 项失败", file=sys.stderr)
        return 1
    enumerated = sum(1 for resource in resources if resource.enumerated)
    print(
        f"check_resources: 资源条目 {len(rows)} 条，覆盖随仓资源 {len(resources)} 个"
        f"（逐条登记 {enumerated} 个），许可与 REUSE.toml 一致"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
