<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# assets/branding — 多平台共享品牌图形

**唯一源是 `hux.png`**（1600×1600 RGBA 的「虎」字艺术图，本仓自绘：近白字形 + 暖橙辉光）； \
其余文件都由它派生，**勿手改派生文件**：

```sh
python3 tools/generators/gen_branding_icons.py
```

生成器只做**版式归一**，不重绘、不降画质：取可见内容（alpha 高于 3%，忽略几乎不可见的辉光尾） \
的外接正方形 + 四周 8% 留白、内容居中（当前画布 `viewBox="237 221 1116 1116"`），再据此派生：

| 文件 | 形态 | 用途 |
|---|---|---|
| `hux.png` | 1600×1600 RGBA，**主源** | 各平台按需另生成尺寸 / 格式；本仓不安装它 |
| `hux.svg` | 自包含：`viewBox` = 归一化画布，主源以 data URI **逐字节内嵌** | 支持 SVG 的主题（缩小渲染到任意尺寸都清晰；放大超过 1600 px 才会软化） |
| `hux-22.png` / `hux-48.png` | 22 / 48 px RGBA，同一画布 Lanczos 缩小 | 位图主题（或缩放器不可用）时的回退 |

**取用指纹**（四个文件的聚合 sha256，`tools/checks/check_branding_assets.py` 在 CI 里核对）：

```sh
cd assets/branding && sha256sum hux.png hux.svg hux-22.png hux-48.png | sha256sum
```

守卫核对：主源 sha256 与常量一致；`hux.svg` 自包含、且内嵌的正是当前主源； \
位图边长与文件名一致（主源与位图都带 alpha）；四文件指纹一致；有 `rsvg-convert` 时再查 SVG 可渲染。 \
**不逐字节比对渲染结果**：不同 librsvg 版本渲染同一 SVG 的字节可以不同， \
逐字节会把版本差异误报成回归。改动图标后， \
把生成器打印的 `MASTER_SHA256` / `CANVAS` / `FINGERPRINT` 更新进守卫即可。

## 各平台取用

| 平台 | 取用方式 |
|---|---|
| Linux（fcitx5） | 安装为 `share/icons/hicolor/{scalable,48x48,22x22}/apps/hux.{svg,png}`<br>输入法条目与状态区按主题名 `hux` 解析 |
| Android | 插件 `assets/` 携带同源图形（按所需密度另生成尺寸） |
| Windows / macOS | 同源派生（Windows 可另生成 `.ico`） |

新增尺寸或格式时：**先在本目录落地并把它加进生成器的 `SIZES`**， \
再由各平台安装规则取用——不要让各平台各自维护一份图形。
