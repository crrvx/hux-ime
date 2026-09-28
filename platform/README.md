<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# platform：平台层

- 分层：`hux-core` → `hux-cfg` / `hux-ffi` → `hux-scheme/tiger` → `platform/*`
- 内核无平台假设，只经 `hux-ffi` 边界接入
- 平台优先级：linux / android → windows → macos / ios

## 状态总览

| 平台 | 状态 | 落点 | 参考实现 |
| --- | --- | --- | --- |
| Linux 桌面 | 构建 / 安装可用，<br>打包待做 | [`fcitx5/`](fcitx5/README.md)<br>+ [`linux/`](linux/README.md) | fcitx5 |
| Android | 待启动（M0 起） | [`fcitx5/`](fcitx5/README.md)<br>+ [`android/`](android/README.md) | fcitx5-android |
| Windows | 未建目录（暂缓） | — | 虎爪（tigerclaw） |
| macOS | 未建目录（暂缓） | — | fcitx5-macos |
| iOS | 未建目录（暂缓） | — | fcitx5-ios |

- **共用关系**：`platform/fcitx5/` 是共享适配层（装配 + C++ 壳），桌面与 Android 都用它 \
  平台专有的事在落点：目录根规则、打开目录、编译装配
- `linux/` 与 `android/` 是两个落点各自的接线、平台实现与文档
- Windows / macOS / iOS **暂缓且不建占位目录**——一句「预留目录」不承载信息； \
  恢复实现时按 [`../docs/design.md`](../docs/design.md) §5 的要求补册

## 各册

- [`fcitx5/README.md`](fcitx5/README.md)：共享适配层契约 \
  （装配、选项角色、按键与提交、反查、配置写入、状态菜单、生命周期、版本下限）
- [`linux/README.md`](linux/README.md)：Linux 桌面落点——入口、安装落点、数据目录、CI 与限制
- [`android/README.md`](android/README.md)：Android 落点—— \
  插件计划、数据与 API 需求、构建计划、验收与里程碑

## 相关文档

- 构建 / 安装 / 卸载见 [`../docs/install.md`](../docs/install.md) \
  安装去向与资源见 [`../docs/resources.md`](../docs/resources.md)
- 配置项见 [`../docs/config.md`](../docs/config.md)
- 随包数据见 [`../data/README.md`](../data/README.md)
- 结构硬规则见 [`../docs/design.md`](../docs/design.md) §1 \
  数据与目录解析见 [`../docs/reference.md`](../docs/reference.md) §2
