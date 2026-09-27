#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
# SPDX-License-Identifier: GPL-3.0-or-later

# 公开入口：Linux 落点的安装脚本在 platform/linux/（见 platform/linux/README.md）。
# 保留 `./install.sh` 这条路，是为了让文档与习惯用法不变；逻辑只有一份。
set -euo pipefail
exec bash "$(cd "$(dirname "$0")" && pwd)/platform/linux/install.sh" "$@"
