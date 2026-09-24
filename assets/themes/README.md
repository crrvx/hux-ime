<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# assets/themes — 跨平台共享主题资源（fcitx5 主题形态）

19 套主题，取自**虎符（hufu-ime-rust）官方皮肤**的 fcitx5 主题转换产物，**原样入库**：
每套一个目录，含 `theme.conf` 与 `panel.png` / `highlight.png` / `prev.png` / `next.png` /
`arrow.png` / `radio.png`。`theme.conf` 顶部的注释即其生成说明（由虎符仓库的皮肤 JSON 转换而来）——
本仓不修改这些文件，改动一律回到生成侧。

- **取用版本**：虎符仓库 `LeafHW/hufu-ime-rust` 的 `54c0339`（`platform/linux/themes/` 全量 19 套）。
- **主题一览**（目录名 → `theme.conf` 的 `Name`）：`canghai` 沧海、`chenwu` 晨雾、`default` 迷雾、
  `huguang` 湖光、`hupo` 琥珀、`luoxia` 落霞、`mocha` 抹茶、`moyan` 墨岩（极简黑）、`mushan` 暮山紫、
  `ouhe` 藕荷、`qingci` 青瓷、`rongyan` 熔岩（炭黑焰橙）、`shiyou` 柿柚、`songyan` 松烟、`sujian` 素笺、
  `xingyu` 杏雨、`xuanmo` 玄墨、`yingxiong` 樱色（暖粉）、`yuebai` 月白。
- **许可**：随虎符仓库根 `LICENSE`（GPL-3.0）；版权与许可标注见仓库根 `REUSE.toml`。
- **取用指纹**（133 个文件的聚合 sha256，`tools/checks/check_themes.py` 在 CI 里核对）：
  `7ad673c4c6df5330db8fc84566a65b93ab39c6686de12428f7caa208208c7a9d`

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
