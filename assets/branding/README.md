<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# assets/branding — 多平台共享品牌图形

**唯一源是 `hux.svg`**（纯几何、不含文字 ⇒ 无字体依赖，16–48 px 下均可辨识）。
各尺寸位图都由它生成，**不要手改位图**：

```sh
rsvg-convert -w 22 -h 22 assets/branding/hux.svg -o assets/branding/hux-22.png
rsvg-convert -w 48 -h 48 assets/branding/hux.svg -o assets/branding/hux-48.png
```

`tools/checks/check_branding_assets.py` 校验「矢量源存在 + 位图尺寸与文件名一致 + 有渲染器时 SVG 可渲染」，
已接入 CI。它**不做逐字节比对**：不同 librsvg 版本的渲染字节可能不同，逐字节会把版本差异误报成回归。

## 各平台取用

| 平台 | 取用方式 |
|---|---|
| Linux（fcitx5） | 安装为 `share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`，输入法条目与状态区按主题名 `hux` 解析 |
| Android | 插件 `assets/` 携带同源图形（按所需密度另生成尺寸） |
| Windows / macOS | 同源派生（Windows 可另生成 `.ico`） |

新增尺寸或格式时：**先在本目录落地并补上生成命令**，再由各平台安装规则取用——不要让各平台各自维护一份图形。
