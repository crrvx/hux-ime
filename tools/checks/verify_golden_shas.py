#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""校验 `goldens/regenerate.md` 的校验和表与金样内部头部。

> 表与 pin 声明在 `goldens/regenerate.md`（`goldens/README.md` 只留清单 / transcript 格式 /
> 校验入口 / 规则），
> 校验项与强度不变。

三件事，任一不符即 `exit 1`：

1. **表 ↔ 文件**：`regenerate.md`「数据夹具」「已入库金样 sha256」两张表里每一条 `| 文件 | sha256 |`
   都按候选根（仓库根 / `goldens/` / `goldens/lexicon/`）唯一解析到实际文件并逐字节比对；
   且顶层金样（`goldens/*.tsv.gz`、`goldens/ngram_fixture.bin`）**必须**都在表里（防新增未登记）。
2. **内部头部 ↔ 表 / 文档声明的 pin**：四份探针 / 表金样（`key`、`key_sequence`、
   `key_sequence_tab`、`sound_to_char_shape`）头部的 `# reference: … @ <pin>` 与 `<来源文件> sha256:` 必须与
   `regenerate.md`「来源与校验和」声明的 pin / sha 一致（换 pin 重生成后只改表、不改头部即失败）。
3. **参照仓库文件 ↔ pin**（`--reference DIR`，需要参照检出）：`lua/*`、`tools/*` 行按该行声明的
   pin 用 `git show <pin>:<path>` 取内容比对；「两 pin 相同」的行两个 pin 都必须相符。

用法：
    python3 tools/checks/verify_golden_shas.py                 # 本地文件 + 头部（无需网络）
    python3 tools/checks/verify_golden_shas.py --reference _external/tiger-sentense-rime
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import re
import subprocess
import sys
from pathlib import Path

# sha 表与 pin 声明所在文档。
SHA_DOC = Path("goldens/regenerate.md")
SHA256_RE = re.compile(r"\b[0-9a-f]{64}\b")
SHA1_RE = re.compile(r"\b[0-9a-f]{40}\b")
TOP_LEVEL_GLOBS = ("*.tsv.gz", "ngram_fixture.bin")
# 四份「CI 不重生成」的金样：头部必须自述 pin 与来源文件 sha256。
# pin 名对应 SHA_DOC「来源与校验和」声明的两个参照 pin + 键名表的 librime pin。
PROBE_HEADERS: dict[str, dict[str, object]] = {
    "goldens/key.tsv.gz": {
        "pin": "librime",
        "sha256": [("key_table.cc sha256", None)],  # None = 取 SHA_DOC 键名表行的 sha
    },
    "goldens/key_sequence.tsv.gz": {
        "pin": "main",
        "sha256": [("tiger_sentence.lua sha256", "lua/tiger_sentence.lua（主干金样）")],
    },
    # Tab 锁路径金样：夹具 `tab_learning: true`，
    # 但参照源码与 pin 同主干，故同样按「主干金样」核对。
    "goldens/key_sequence_tab.tsv.gz": {
        "pin": "main",
        "sha256": [("tiger_sentence.lua sha256", "lua/tiger_sentence.lua（主干金样）")],
    },
    "goldens/sound_to_char_shape.tsv.gz": {
        "pin": "reverse",
        "sha256": [
            ("tiger_sentence.lua sha256", "lua/tiger_sentence.lua（音反查金样）"),
            ("PY_c.dict.yaml sha256", "sound_to_char_shape/PY_c.dict.yaml"),
        ],
    },
}


class Failure(Exception):
    """一条校验失败（汇总后统一打印）。"""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_tables(text: str) -> tuple[dict[str, str], dict[str, tuple[str, str, str]]]:
    """返回（本仓文件标签 → sha256，参照行标签 → (来源说明, sha256, 仓库内路径)）。"""
    local: dict[str, str] = {}
    reference: dict[str, tuple[str, str, str]] = {}
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 2:
            continue
        shas = SHA256_RE.findall(cells[-1])
        if len(shas) != 1:
            continue
        # 首列形如 `path`（可带括注，如 `...`（主干金样））⇒ 取第一个反引号片段为路径，
        # 整格（去反引号）为行标签——两个 pin 的 `lua/tiger_sentence.lua` 靠括注区分。
        path_match = re.search(r"`([^`]+)`", cells[0])
        if not path_match:
            continue
        path = path_match.group(1)
        label = cells[0].replace("`", "")
        # 参照仓库文件行：首列以 `lua/` / `tools/` 开头（`regenerate.md` 明标「均为参照仓库路径」）。
        if path.startswith(("lua/", "tools/")):
            reference[label] = (cells[1], shas[0], path)
        else:
            local[path] = shas[0]
    return local, reference


def resolve_local(root: Path, label: str) -> Path:
    """按候选根唯一解析表里的文件标签（`lexicon/` 段的行是相对 `goldens/lexicon/` 的）。"""
    candidates = [
        root / label,
        root / "goldens" / label,
        root / "goldens" / "lexicon" / label,
    ]
    existing = [path for path in candidates if path.is_file()]
    if not existing:
        raise Failure(f"表里的文件不存在：{label}（试过 {', '.join(str(p) for p in candidates)}）")
    if len(existing) > 1:
        raise Failure(f"表里的文件标签歧义：{label} 命中 {len(existing)} 处：{existing}")
    return existing[0]


def next_hex(text: str, marker: str, pattern: re.Pattern[str]) -> str:
    """取唯一标记 `marker` 之后出现的第一个十六进制串（校验和文档的 pin 说明跨行折行）。"""
    if text.count(marker) != 1:
        raise Failure(f"{SHA_DOC} 里的标记 {marker} 出现 {text.count(marker)} 次（应为 1 次）")
    index = text.find(marker)
    if index < 0:
        raise Failure(f"{SHA_DOC} 缺少标记：{marker}")
    found = pattern.search(text, index)
    if not found:
        raise Failure(f"{SHA_DOC} 的「{marker}」之后没有 {pattern.pattern} 串")
    return found.group(0)


def header_lines(path: Path) -> list[str]:
    with gzip.open(path, "rt", encoding="utf-8") as handle:
        lines = []
        for line in handle:
            if not line.startswith("#"):
                break
            lines.append(line.rstrip("\n"))
    return lines


def header_value(lines: list[str], key: str) -> str:
    prefix = f"# {key}:"
    for line in lines:
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    raise Failure(f"金样头部缺少 `# {key}:` 行（现有头部：{lines}）")


def header_pin(lines: list[str]) -> str:
    reference = header_value(lines, "reference")
    found = SHA1_RE.search(reference)
    if not found:
        raise Failure(f"金样头部的 `# reference:` 行没有 pin：{reference}")
    return found.group(0)


def check_reference_file(repo: Path, pin: str, path: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), "show", f"{pin}:{path}"],
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        message = result.stderr.decode("utf-8", "replace").strip()
        hint = ""
        if "unknown revision" in message or "Not a valid object name" in message:
            hint = (
                f"（检出里没有 {pin}：CI 用 `git fetch --depth 1 origin <sha>` 逐个取；"
                f"本地检出请 `git fetch origin {pin}`）"
            )
        raise Failure(f"取不到 {path} @ {pin}：{message}{hint}")
    return hashlib.sha256(result.stdout).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="仓库根（默认按脚本位置推断）",
    )
    parser.add_argument(
        "--reference",
        type=Path,
        default=None,
        help="参照仓库本地检出（给定时额外校验 lua/* 与 tools/* 的来源 sha）",
    )
    parser.add_argument("--verbose", action="store_true", help="打印每条通过项")
    args = parser.parse_args()
    root: Path = args.root.resolve()

    failures: list[str] = []
    passed = 0

    def note(ok: bool, ok_message: str, failure: str | None = None) -> None:
        nonlocal passed
        if ok:
            passed += 1
            if args.verbose:
                print(f"  ok  {ok_message}")
        else:
            failures.append(failure if failure is not None else ok_message)

    sha_doc_path = root / SHA_DOC
    if not sha_doc_path.is_file():
        print(f"FAIL 找不到校验和文档：{SHA_DOC}", file=sys.stderr)
        return 1
    text = sha_doc_path.read_text(encoding="utf-8")
    local, reference = parse_tables(text)
    pins = {
        "main": next_hex(text, "**主干 pin**", SHA1_RE),
        "reverse": next_hex(text, "**反查分支 pin**", SHA1_RE),
        "librime": next_hex(text, "**键名表**", SHA1_RE),
    }
    key_table_sha = next_hex(text, "**键名表**", SHA256_RE)
    declared_shas: dict[str, str] = dict(local)
    declared_shas.update({label: sha for label, (_, sha, _) in reference.items()})

    # 1. 表 ↔ 文件
    for label, sha in sorted(local.items()):
        path = resolve_local(root, label)
        actual = sha256_file(path)
        note(
            actual == sha,
            f"{path.relative_to(root)} sha256 与 {SHA_DOC} 表一致",
            f"{path.relative_to(root)} sha256 与 {SHA_DOC} 表不符：表 {sha}，实际 {actual}",
        )

    # 1b. 顶层金样必须全部登记（新增未登记即失败）
    for pattern in TOP_LEVEL_GLOBS:
        for path in sorted((root / "goldens").glob(pattern)):
            note(
                path.name in local,
                f"{path.relative_to(root)} 已登记在 {SHA_DOC} 的 sha256 表中",
                f"{path.relative_to(root)} 未登记在 {SHA_DOC} 的 sha256 表中",
            )
    note(
        bool(local),
        f"{SHA_DOC} 的 sha256 表解析出 {len(local)} 行",
        f"{SHA_DOC} 的 sha256 表为空（解析失败？）",
    )

    # 2. 内部头部 ↔ 表 / 声明的 pin
    for label, spec in sorted(PROBE_HEADERS.items()):
        path = root / label
        try:
            lines = header_lines(path)
            pin = header_pin(lines)
            wanted_pin = pins[str(spec["pin"])]
            note(
                pin == wanted_pin,
                f"{label} 头部 pin {pin} 与 {SHA_DOC} 声明的 {spec['pin']} pin 一致",
                f"{label} 头部 pin {pin} 与 {SHA_DOC} 声明的 {spec['pin']} pin {wanted_pin} 不符",
            )
            for key, table_label in spec["sha256"]:  # type: ignore[union-attr]
                declared = header_value(lines, str(key))
                expected = (
                    key_table_sha if table_label is None else declared_shas[str(table_label)]
                )
                note(
                    declared == expected,
                    f"{label} 头部 `{key}` 与 {SHA_DOC} 表一致",
                    f"{label} 头部 `{key}` = {declared}，与 {SHA_DOC} 表的 {expected} 不符",
                )
        except Failure as error:
            failures.append(f"{label}: {error}")

    # 3. 参照仓库文件 ↔ 各自声明的 pin（可选）
    if args.reference is not None:
        reference_repo = args.reference.resolve()
        if not (reference_repo / ".git").exists():
            failures.append(f"--reference 不是 git 检出：{reference_repo}")
        else:
            for label, (source, sha, path) in sorted(reference.items()):
                if "反查" in source:
                    wanted = ["reverse"]
                elif "主干" in source:
                    wanted = ["main"]
                elif "两 pin 相同" in source:
                    wanted = ["main", "reverse"]
                else:
                    failures.append(f"{label} 的来源说明无法判定 pin：{source!r}")
                    continue
                for name in wanted:
                    pin = pins[name]
                    try:
                        actual = check_reference_file(reference_repo, pin, path)
                    except Failure as error:
                        failures.append(str(error))
                        continue
                    note(
                        actual == sha,
                        f"{path} @ {name} pin {pin[:7]} sha256 与 {SHA_DOC} 表一致",
                        f"{path} @ {name} pin {pin[:7]} sha256 不符：{SHA_DOC} 表 {sha}，检出 {actual}",
                    )

    for message in failures:
        print(f"FAIL {message}", file=sys.stderr)
    scope = "本地文件 + 金样头部" + (" + 参照检出" if args.reference is not None else "")
    print(f"verify_golden_shas: {passed} 项通过，{len(failures)} 项失败（{scope}）")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
