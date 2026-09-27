#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later
"""守卫脚本的共享助手：仓库根推导与失败出口。

各脚本以 `python3 tools/checks/<名>.py` 直接运行，`sys.path[0]` 即本目录，
故 `import _common` 无需引导（`tools/generators/` 下的生成器跨目录使用时才要显式加路径）。
"""

from __future__ import annotations

import sys
from collections.abc import Callable
from pathlib import Path
from typing import NoReturn


def repo_root() -> Path:
    """仓库根：本文件在 `<根>/tools/checks/`，故上两级即根。"""
    return Path(__file__).resolve().parents[2]


def fail_for(script: str) -> Callable[[str], NoReturn]:
    """把失败出口绑定到脚本名：`<脚本名>: <消息>` 写 stderr 后**立即退出**（退出码 1）。

    绑定后调用点仍是 `fail("<消息>")`，各守卫的失败格式于是只有这一处实现。
    **累计**失败项（先记完再统一总结）的脚本不适用立即退出，见 `check_resources.py` 的
    局部实现：它只打印，退出码由 `main` 汇总返回。
    """

    def fail(message: str) -> NoReturn:
        print(f"{script}: {message}", file=sys.stderr)
        raise SystemExit(1)

    return fail
