<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform/linux

fcitx5 **桌面（Linux）** 适配：构建入口、安装与打包。

- 构建/安装（手工）：`cmake -S platform/fcitx5 -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr`
  → `cmake --build build/addon -j` → `sudo cmake --install build/addon`；
- 一键：仓库根 `./install.sh` / `./uninstall.sh`（预演 `--dry-run`）；
- 随包数据（`data/MANIFEST`）：`cmake --install` 与 `install.sh` 都装到 `<prefix>/share/fcitx5/hux/`
  （卸载按同一清单；自检 `tools/checks/check_data_manifest.sh`）。**模型不随包**；
- 数据目录：`$XDG_DATA_HOME/fcitx5/hux` 与 `$XDG_DATA_DIRS/*/fcitx5/hux`（解析见 `platform/fcitx5/src/paths.rs`），
  插件库目录兼容 `lib` / `lib64`（`FCITX_INSTALL_ADDONDIR` 优先）。
- 共享实现见 [`../fcitx5/`](../fcitx5/README.md)；Android 见 [`../android/`](../android/README.md)。
- CI：`addon` 作业覆盖 configure / 构建 / 链接、`hux_abi.h` ↔ `libhux.so` 符号一致、
  `DESTDIR` 安装布局（3 个插件文件 + `data/MANIFEST` 全部随包数据）。
- 状态：构建/安装可用；**打包（PKGBUILD）待做**。
