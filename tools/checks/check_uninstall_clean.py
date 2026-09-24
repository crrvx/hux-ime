#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""卸载守卫：`cmake --install` 装出的每个文件都必须有归宿（被卸载、或按设计保留）。

    python3 tools/checks/check_uninstall_clean.py \
        --manifest build/addon/install_manifest.txt --stage /tmp/stage

判据：**安装集合 ⊆（可卸载集合 ∪ 保留表）**

- 安装集合：`cmake --install` 写出的 `install_manifest.txt`（权威安装集合）。`--stage` 给出
  `DESTDIR`：清单里若带该前缀（`$DESTDIR/usr/...`）就摘掉，记的是逻辑安装路径 `/usr/...`；
  不给 `--stage` 即视为无 DESTDIR。CMake 各版本对清单是否带 DESTDIR 前缀并不一致（本机 4.4.3
  不带），两种都接受。
- 可卸载集合：`uninstall.sh --dry-run` 输出的「计划删除清单」里 `-` / `?` 两种前缀行，取标记后的
  路径。口径：`-` = 按缺省就删，`?` = 需交互回答 y 才删（模型 / 用户数据这类缺省保留的项）；
  两者都算**可卸载**——守卫只问「这条路有没有归宿」，不问「要不要多问一句」。行格式（与脚本
  头注释同一契约）：绝对路径一行一条，以 `/` 结尾表示整个目录，含 `*` / `?` 的模式按 fnmatch
  匹配；清单不上色（本守卫按行首两字符解析，人读信息照常打印），且**在伪终端里复跑一次**验证
  「--dry-run 不上色」——管道天生无色，只有终端这条路径能看出着色。
- 保留表：本文件的 `RETAINED` 常量（**当前为空**），只登记「卸载脚本永远不会删除」的已安装路径。
  被 `?` 行覆盖的路径属可卸载集合，不进保留表——否则某条 `?` 行回归时会由保留表兜住，守卫看不出
  来。保留表只解释「为什么这不是漏删」，不能用来掩盖遗忘：路径被判为保留时仍要写明归类。

失败时逐条列出「没有任何归宿」的已安装路径，并指明处置：先改 `uninstall.sh --dry-run` 的计划清单
覆盖该路径；若该路径确属按设计保留，登记进 `RETAINED` 并写清原因。
"""

from __future__ import annotations

import argparse
import fnmatch
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# `uninstall.sh --dry-run` 计划清单的两种行首标记（契约见该脚本头注释）：
# `-` 按缺省就删、`?` 交互回答 y 才删；两者都算可卸载。
MARKERS: dict[str, str] = {"-": "按缺省删", "?": "交互回答 y 才删"}

# 按设计保留的类别（fnmatch 模式 + 原因）：只登记**卸载脚本永远不会删除**的安装路径。
# 与 `uninstall.sh --dry-run` 的 `?` 行分工：「?」= 交互回答 y 就删，属可卸载集合，由守卫按
# 覆盖面校验；这里只解释「为什么某条已安装路径不在可卸载集合里也不算漏删」。当前为空：
# 模型与用户数据都在 `?` 行里声明，随包数据 / 插件 / 图标 / 主题都在 `-` 行里。
RETAINED: tuple[tuple[str, str], ...] = ()


def fail(message: str) -> None:
    print(f"卸载守卫失败：{message}", file=sys.stderr)
    raise SystemExit(1)


def read_manifest(path: Path, stage: str) -> list[str]:
    """读 `cmake --install` 的安装清单，按 `--stage` 摘掉可能存在的 DESTDIR 前缀。"""
    if not path.is_file():
        fail(f"缺少安装清单 {path}（先在同一个 build 目录跑 `cmake --install`）")
    prefix = stage.rstrip("/")
    installed: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if prefix and line.startswith(prefix + "/"):
            line = line[len(prefix) :]
        if not line.startswith("/"):
            fail(f"清单路径不是绝对路径：{line}")
        installed.append(line)
    if not installed:
        fail(f"安装清单 {path} 没有有效路径（安装是不是没跑成？）")
    return installed


def uninstall_surface(script: Path) -> list[tuple[str, str]]:
    """跑 `uninstall.sh --dry-run`，取「计划删除清单」里的 `-` / `?` 行（返回 标记 + 路径）。"""
    if not script.is_file():
        fail(f"缺少卸载脚本 {script}")
    result = subprocess.run(
        ["bash", str(script), "--dry-run"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        fail(f"{script.name} --dry-run 退出码 {result.returncode}：{detail}")
    surface: list[tuple[str, str]] = []
    for raw in result.stdout.splitlines():
        line = raw.strip()
        if len(line) < 3 or line[0] not in MARKERS or line[1] != " ":
            continue
        if "\x1b" in line:
            fail(f"--dry-run 的清单行带 ANSI 转义（约定不上色）：{line!r}")
        path = line[2:]
        if not path.startswith("/"):
            fail(f"--dry-run 的清单行不是绝对路径：{line!r}")
        surface.append((line[0], path))
    if not surface:
        fail(f"{script.name} --dry-run 没有输出「计划删除清单」（可卸载集合为空）")
    return surface


def uncolored_on_tty(script: Path) -> str:
    """在伪终端里复跑 `--dry-run`，验证「预览不上色」的契约（管道天生无色，看不到这条）。

    着色只在终端上出现，所以只在 `script`（util-linux）可用时验证；不可用或参数不受支持就跳过
    ——契约本身由 `uninstall.sh` 的 `--dry-run` 分支保证，这一步只是把「人读终端」也纳入验证。
    """
    if shutil.which("script") is None:
        return "跳过（没有 script 命令）"
    env = {key: value for key, value in os.environ.items() if key != "NO_COLOR"}
    command = f"bash {shlex.quote(str(script))} --dry-run"
    result = subprocess.run(
        ["script", "-qec", command, "/dev/null"],
        cwd=ROOT,
        env=env,
        capture_output=True,
    )
    if result.returncode != 0:
        return "跳过（伪终端复跑失败，可能是 script 不支持 -qec）"
    if b"\x1b" in result.stdout:
        fail(f"{script.name} --dry-run 在终端下带上了 ANSI 转义（契约：预览不上色）")
    return "终端（伪 TTY）下无 ANSI"


def covered(path: str, surface: list[tuple[str, str]]) -> bool:
    for _, entry in surface:
        if entry.endswith("/"):
            if path.startswith(entry):
                return True
        elif "*" in entry or "?" in entry or "[" in entry:
            if fnmatch.fnmatchcase(path, entry):
                return True
        elif path == entry:
            return True
    return False


def retained_reason(path: str) -> str | None:
    for pattern, reason in RETAINED:
        if fnmatch.fnmatchcase(path, pattern):
            return reason
    return None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="校验 cmake --install 装出的文件都落在 uninstall.sh --dry-run 的可卸载集合里，或按设计保留",
    )
    parser.add_argument("--manifest", required=True, help="cmake --install 的 install_manifest.txt")
    parser.add_argument("--stage", default="", help="DESTDIR（清单路径的前缀）；缺省表示无 DESTDIR")
    parser.add_argument(
        "--uninstall",
        default=str(ROOT / "uninstall.sh"),
        help="被检查的卸载脚本（缺省仓库根 uninstall.sh）",
    )
    args = parser.parse_args()

    installed = read_manifest(Path(args.manifest), args.stage)
    surface = uninstall_surface(Path(args.uninstall))
    tty_note = uncolored_on_tty(Path(args.uninstall))

    covered_paths: list[str] = []
    retained: list[tuple[str, str]] = []
    orphans: list[str] = []
    for path in installed:
        reason = retained_reason(path)
        if reason is not None:
            retained.append((path, reason))
        elif covered(path, surface):
            covered_paths.append(path)
        else:
            orphans.append(path)

    if orphans:
        print(
            "以下已安装路径没有任何归宿（既不在 uninstall.sh --dry-run 计划清单的 - / ? 行里，"
            "也不在保留表里）：",
            file=sys.stderr,
        )
        for path in orphans:
            print(f"  · {path}", file=sys.stderr)
        print(
            "处置：先改 uninstall.sh 的 print_plan，让计划清单覆盖这些路径（按缺省删给 - 行，"
            "交互回答 y 才删给 ? 行）；若确属「按设计保留」，把它登记进本文件 RETAINED 常量表并写清原因。",
            file=sys.stderr,
        )
        return 1

    counts = {marker: sum(1 for mark, _ in surface if mark == marker) for marker in MARKERS}
    print(
        f"卸载守卫通过：安装 {len(installed)} 项："
        f"卸载覆盖 {len(covered_paths)} 项、按设计保留 {len(retained)} 项"
    )
    print(
        "  可卸载集合（uninstall.sh --dry-run 计划清单）："
        + "、".join(f"{marker} 行 {counts[marker]} 条（{MARKERS[marker]}）" for marker in sorted(MARKERS))
        + f"，共 {len(surface)} 条"
    )
    print(f"  预览着色：{tty_note}")
    for path, reason in retained:
        print(f"  按设计保留 {path}（{reason}）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
