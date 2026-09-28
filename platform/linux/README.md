<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Linux 桌面：fcitx5 落点

- 本册只写 Linux 桌面特有的部分：入口脚本、安装落点、数据目录、CI 与限制
- 宿主层契约（选项角色、按键与提交、反查、配置写入、生命周期）在 \
  [`../fcitx5/README.md`](../fcitx5/README.md)，桌面与 Android 共用
- 状态：构建与安装可用；**发行版打包（PKGBUILD，AUR `fcitx5-hux`）待做**， \
  见 [`../../docs/open-items.md`](../../docs/open-items.md)

## 入口

- 根 [`install.sh`](../../install.sh) 分两级，**互斥**：
  - `-s|--system`（缺省，装 `/usr`，需 sudo）
  - `-u|--user`（装 `$HOME/.local`）
  - 另有 `--dry-run` 与 `-h`
- 脚本装完**不自动重启** fcitx5，按结尾提示自行重启
- 根 [`uninstall.sh`](../../uninstall.sh) 按清单删掉插件、图标、随包数据与 `environment.d` \
  并依次询问共享主题（缺省删）、模型（缺省留）与用户数据（缺省留）
- 逐步命令、依赖包与产物清单见 [`../../docs/install.md`](../../docs/install.md)

## 安装落点

- 插件目录由 fcitx5 决定，本仓只补三条规则：
  - **系统级**跟随 `FCITX_INSTALL_ADDONDIR`：
    - 可能是 `lib64/fcitx5` 或 multiarch 的 `lib/<triplet>/fcitx5`
    - 本仓对 `lib` 与 `lib64` 都兼容
  - **用户级**加 `-DHUX_RELATIVE_ADDON_DIR=ON`，把安装目标记为**相对**路径 `lib/fcitx5` \
    该变量本身取的是绝对路径，`--prefix` 无法重定位，实际落点 `<prefix>/lib/fcitx5/libhux.so`
  - 插件目录**没有用户级缺省值**，`-u` 依赖 `FCITX_ADDON_DIRS` \
    - [`install.sh`](../../install.sh) 会写 `environment.d` 并检测是否生效
    - 回退办法见 [`../../docs/install.md`](../../docs/install.md)「用户级（`-u`）的环境变量」
- 插件之外的落点（数据、主题、图标、conf）见 \
  [`../../docs/resources.md`](../../docs/resources.md)「落点与查找顺序」
- 手动安装等价于 `cmake --install`，产物是：
  - 3 个插件文件 + 3 个图标，另加
  - [`../../data/MANIFEST`](../../data/MANIFEST) 列出的全部随包数据

## 数据目录

- 查找顺序：`HUX_DATA_DIRS` → `XDG_DATA_HOME` → `XDG_DATA_DIRS` → `/usr/share` \
  两个覆盖变量见 [`../../docs/reference.md`](../../docs/reference.md) §2
- 根规则（XDG）在本落点 `src/lib.rs`；子目录与排序在 `fcitx5/src/paths.rs` \
  内核**不读环境变量**，环境变量只在平台层解析
- 「打开模型目录」也在本落点：`shell/open_directory.cpp` 用 `fork` + `xdg-open` \
  （失败退 `gio open`）交给文件管理器；共用层只声明 `fcitx5/shell/platform.h`
- n-gram 模型**不随包**：从上游 release 获取后，放置位置：
  - 用户级 `~/.local/share/fcitx5/hux/models/`
  - 系统级 `/usr/share/fcitx5/hux/models/`
  > 不装也能用，只是整句排序略弱

## CI

- `addon` 作业在 **ubuntu-26.04** 上用 apt 的 fcitx5，**不钉版本**（现为 5.1.19），
  走完 configure → 构建链接 → C ABI 符号比对 → `DESTDIR` 安装布局校验
- `ubuntu-latest` 目前仍是 24.04，apt 只给 5.1.7（低于下限）；等迁到 26 后换回该标签
- 作业定义见 [`../../.github/workflows/ci.yml`](../../.github/workflows/ci.yml) \
  测试与性能纪律见 [`../../docs/design.md`](../../docs/design.md) §3

## 限制

- **打包待做**：PKGBUILD / AUR 未落地
- 卸载洁净由 `tools/checks/check_uninstall_clean.py` 守护（CI 已接入） \
  见 [`../../docs/install.md`](../../docs/install.md)「卸载（无残留）」
