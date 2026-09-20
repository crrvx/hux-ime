<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform/linux

fcitx5 **桌面（Linux）** 适配：构建入口、安装与打包。

- 构建/安装（手工）：`cmake -S platform/fcitx5 -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr`
  → `cmake --build build/addon -j` → `sudo cmake --install build/addon`；
- 一键：仓库根 `./install.sh` / `./uninstall.sh`（预演 `--dry-run`）；
- 数据目录：`$XDG_DATA_HOME/fcitx5/hux` 与 `$XDG_DATA_DIRS/*/fcitx5/hux`（解析见 `platform/fcitx5/src/paths.rs`）。
- 共享实现见 [`../fcitx5/`](../fcitx5/README.md)；Android 见 [`../android/`](../android/README.md)。
- CI：`addon` 作业覆盖 configure / 构建 / 链接、`hux_abi.h` ↔ `libhux.so` 符号一致、`DESTDIR` 安装布局。
- 状态：构建/安装可用；**打包（PKGBUILD）待做**。
