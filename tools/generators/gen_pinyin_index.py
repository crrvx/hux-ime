#!/usr/bin/env python3
"""音查虎索引生成器（⑧-1）：PY_c.dict.yaml → TCSRV01 紧凑索引。

语义依据（librime 1.17.0 的词典反查；本项目称「音查虎」，见 docs/rust-migration.md）：
- 音节表 = 码列按空格切分的 token 去重，**字典序**（librime `Syllabary = set<string>`）；
- 拼写表 = 音节本体 + 缩写（PY_c.schema.yaml 的 `speller/algebra`：
  `abbrev/^([a-z]).+$/$1/`、`abbrev/^[zcs]h.+$/$1/`）；缩写可信度罚 log(0.5)、
  补全罚 log(0.05) 由运行时施加，索引只记录类型；
- 词条按「码（音节 id 序列）」分组，组内按权重降序（稳定；等同 `SortHomophones`）。

用法：
  tools/generators/gen_pinyin_index.py --source PY_c.dict.yaml --out data/tiger_sentence.pinyin.bin.gz
  tools/generators/gen_pinyin_index.py --source PY_c.dict.yaml --out ... --manifest docs/PINYIN_INDEX_MANIFEST.json
  tools/generators/gen_pinyin_index.py --check --source PY_c.dict.yaml --out ...

二进制布局（小端；`u16/u32` 定长）：
  magic[8] = "TCSRV01\\n"
  u32 syllable_count, u32 spelling_count, u32 group_count, u32 entry_count
  syllables: [u16 len + bytes]（字典序，id = 下标）
  spellings: [u16 len + bytes + u8 alt_count + alt_count × (u32 syllable_id + u8 type)]
             （按 len+bytes 字节序排序；type: 0=normal 1=abbrev）
  groups:    [u8 syl_count + syl_count × u16 syllable_id + u32 entry_count]
             （按 code 字典序排序：逐项比较、短者在前；前缀连续）
  entries:   [u32 weight + u16 text_len + bytes]（组序、组内权重降序）
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import pathlib
import re
import struct
import sys

MAGIC = b"TCSRV01\n"
TYPE_NORMAL = 0
TYPE_ABBREV = 1
ABBREV_RULES = (
    re.compile(r"^([a-z]).+$"),          # PY_c.schema.yaml: abbrev/^([a-z]).+$/$1/
    re.compile(r"^([zcs]h).+$"),         # PY_c.schema.yaml: abbrev/^([zcs]h).+$/$1/
)


class Entry:
    __slots__ = ("text", "code", "weight", "order")

    def __init__(self, text: str, code: tuple[str, ...], weight: int, order: int) -> None:
        self.text = text
        self.code = code
        self.weight = weight
        self.order = order


def parse_columns(header: list[str]) -> dict[str, int]:
    """解析 dict.yaml 头部的 columns 列表（`- text #字词`）。"""
    columns: dict[str, int] = {}
    in_columns = False
    index = 0
    for line in header:
        stripped = line.strip()
        if stripped.startswith("columns:"):
            in_columns = True
            continue
        if in_columns:
            if stripped.startswith("- "):
                name = stripped[2:].split("#", 1)[0].strip()
                columns[name] = index
                index += 1
            elif stripped and not stripped.startswith("#"):
                break
    if not columns:
        raise SystemExit("dict header missing columns")
    return columns


def read_dict(path: pathlib.Path) -> tuple[list[Entry], str]:
    header: list[str] = []
    rows: list[str] = []
    with path.open(encoding="utf-8") as handle:
        in_header = True
        for line in handle:
            if in_header:
                if line.startswith("..."):
                    in_header = False
                else:
                    header.append(line)
                continue
            line = line.rstrip("\n")
            if line:
                rows.append(line)
    columns = parse_columns(header)
    sort_order = "by_weight"
    for line in header:
        if line.strip().startswith("sort:"):
            sort_order = line.split(":", 1)[1].strip()
    text_col = columns.get("text")
    code_col = columns.get("code")
    weight_col = columns.get("weight")
    if text_col is None:
        raise SystemExit("dict header missing text column")
    entries: list[Entry] = []
    for order, row in enumerate(rows):
        fields = row.split("\t")
        if len(fields) <= text_col or not fields[text_col]:
            continue
        text = fields[text_col]
        code = tuple(fields[code_col].split()) if code_col is not None and len(fields) > code_col else ()
        weight = 0
        if weight_col is not None and len(fields) > weight_col and fields[weight_col]:
            weight_text = fields[weight_col]
            try:
                weight = int(weight_text)
            except ValueError:
                raise SystemExit(f"非整数权重：{text!r} -> {weight_text!r}（生成器暂只支持整数）")
        entries.append(Entry(text, code, weight, order))
    return entries, sort_order


def build_index(entries: list[Entry], sort_order: str) -> dict:
    syllables = sorted({syllable for entry in entries for syllable in entry.code})
    syllable_id = {syllable: index for index, syllable in enumerate(syllables)}

    spellings: dict[str, list[tuple[int, int]]] = {}
    for index, syllable in enumerate(syllables):
        spellings.setdefault(syllable, []).append((index, TYPE_NORMAL))
        for rule in ABBREV_RULES:
            match = rule.match(syllable)
            if match:
                spellings.setdefault(match.group(1), []).append((index, TYPE_ABBREV))

    groups: dict[tuple[int, ...], list[Entry]] = {}
    for entry in entries:
        code = tuple(syllable_id[syllable] for syllable in entry.code)
        groups.setdefault(code, []).append(entry)
    for code, group in groups.items():
        if sort_order == "by_weight":
            group.sort(key=lambda item: (-item.weight, item.order))
        else:
            group.sort(key=lambda item: item.order)

    return {
        "syllables": syllables,
        "spellings": [ (key, sorted(alts)) for key, alts in sorted(spellings.items()) ],
        # 码序：字典序（前缀连续，供运行时长前缀检索/精确查找）；同 librime 的检索语义无关。
        "groups": sorted(groups.items(), key=lambda item: item[0]),
    }


def encode(index: dict) -> bytes:
    out = bytearray()
    syllables = index["syllables"]
    spellings = index["spellings"]
    groups = index["groups"]
    out += MAGIC
    out += struct.pack("<IIII", len(syllables), len(spellings), len(groups),
                       sum(len(entries) for _, entries in groups))

    def put_bytes(value: bytes) -> None:
        out.extend(struct.pack("<H", len(value)))
        out.extend(value)

    for syllable in syllables:
        put_bytes(syllable.encode("utf-8"))
    for key, alts in spellings:
        put_bytes(key.encode("utf-8"))
        out.append(len(alts))
        for syllable, kind in alts:
            out.extend(struct.pack("<IB", syllable, kind))
    for code, entries in groups:
        out.append(len(code))
        for syllable in code:
            out.extend(struct.pack("<H", syllable))
        out.extend(struct.pack("<I", len(entries)))
    for _, entries in groups:
        for entry in entries:
            text = entry.text.encode("utf-8")
            out.extend(struct.pack("<I", entry.weight))
            put_bytes(text)
    return bytes(out)


def write_output(path: pathlib.Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.suffix == ".gz":
        with path.open("wb") as raw:
            # filename="" + mtime=0：输出与路径无关，保证可重现。
            with gzip.GzipFile(filename="", fileobj=raw, mode="wb", compresslevel=9,
                               mtime=0) as gz:
                gz.write(data)
    else:
        path.write_bytes(data)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description="PY_c.dict.yaml → TCSRV01 音查虎索引")
    parser.add_argument("--source", required=True, type=pathlib.Path, help="PY_c.dict.yaml")
    parser.add_argument("--out", required=True, type=pathlib.Path, help="输出（.gz 结尾则 gzip）")
    parser.add_argument("--manifest", type=pathlib.Path, help="写出/校验 manifest（JSON）")
    parser.add_argument("--check", action="store_true", help="只校验 --out 与 manifest 一致")
    parser.add_argument("--repo", default="https://github.com/crrvx/tiger-sentense-rime", help="manifest 记录的源仓库地址（URL）")
    parser.add_argument("--commit", default="", help="manifest 记录的源提交")
    args = parser.parse_args()

    entries, sort_order = read_dict(args.source)
    index = build_index(entries, sort_order)
    data = encode(index)
    counts = {
        "entries": sum(len(group) for _, group in index["groups"]),
        "code_groups": len(index["groups"]),
        "syllables": len(index["syllables"]),
        "spelling_keys": len(index["spellings"]),
    }
    source_sha = sha256(args.source)

    if args.check:
        if not args.out.is_file():
            raise SystemExit(f"missing output: {args.out}")
        actual = sha256(args.out)
        if args.manifest and args.manifest.is_file():
            manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
            expect = manifest["output"]["sha256"]
            if actual != expect:
                raise SystemExit(f"sha256 mismatch: {actual} != {expect}")
            if manifest.get("counts") != counts:
                raise SystemExit(f"counts mismatch: {counts} != {manifest.get('counts')}")
        print(f"check ok: {args.out} sha256={actual}")
        return 0

    write_output(args.out, data)
    output_sha = sha256(args.out)
    print(f"wrote {args.out} ({args.out.stat().st_size} bytes, sha256={output_sha})")
    print("counts", counts)
    if args.manifest:
        manifest = {
            "format": "TCSRV01",
            "generator": "tools/generators/gen_pinyin_index.py",
            "source": {
                "repo": args.repo,
                "commit": args.commit,
                "file": args.source.name,
                "sha256": source_sha,
                "sort": sort_order,
            },
            "counts": counts,
            "output": {
                "file": str(args.out),
                "bytes": args.out.stat().st_size,
                "sha256": output_sha,
            },
        }
        args.manifest.write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        print(f"wrote {args.manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
