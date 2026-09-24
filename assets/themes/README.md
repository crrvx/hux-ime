<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# assets/themes — 跨平台共享主题资源（fcitx5 主题形态）

9 套主题，取自**虎符（hufu-ime-rust）官方皮肤**的 fcitx5 主题转换产物，**原样入库**：
每套一个目录，含 `theme.conf` 与 `panel.png` / `highlight.png` / `prev.png` / `next.png` /
`arrow.png` / `radio.png`。`theme.conf` 顶部的注释即其生成说明（由虎符仓库的皮肤 JSON 转换而来）——
本仓不修改这些文件，改动一律回到生成侧。

- **许可**：随虎符仓库根 `LICENSE`（GPL-3.0）；版权与许可标注见仓库根 `REUSE.toml`。
- **取用指纹**（63 个文件的聚合 sha256，`tools/checks/check_themes.py` 在 CI 里核对）：
  `8da6a24135e4881e50d5b1ef8d8d85691cda35085f3e1f8679f48ce3ddd5039c`

  ```sh
  cd assets/themes && find hufu-* -type f | sort | xargs sha256sum | sha256sum
  ```

## 清单与安装（Linux）

`MANIFEST` 是**单一来源**：`platform/fcitx5/CMakeLists.txt` 按它安装、`install.sh` 装后逐条核对、
`uninstall.sh` 按它删除、`tools/checks/check_themes.py` 守护四者一致。新增/删除主题只改 `MANIFEST`
（并同步指纹）。

安装落点：`<prefix>/share/fcitx5/themes/<主题目录>/`（系统级）。fcitx5 会合并系统级与用户级
（`~/.local/share/fcitx5/themes/`）主题目录；选用：`fcitx5-configtool` →「附加组件」→「经典界面」→ 主题，
或改 `~/.config/fcitx5/conf/classicui.conf` 的 `Theme=`。

## 其它平台

本目录是**共享资源**（配色与图形），fcitx5 主题只是其中一种呈现形态。Windows / macOS / Android
若需要各自的皮肤形态，应从同一份源派生，而不是各自维护一套。

## 更新步骤

1. 在虎符仓库重新生成主题产物；
2. 覆盖本目录（保持目录名与文件集合：`theme.conf` + 6 张 PNG）；
3. 同步 `MANIFEST`；
4. 重算指纹并更新 `tools/checks/check_themes.py` 里的常量；
5. 跑 `python3 tools/checks/check_themes.py` 与 `reuse lint`。
