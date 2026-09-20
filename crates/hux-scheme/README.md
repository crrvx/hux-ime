<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# crates/hux-scheme

输入方案（码表 + 解码 + 学习规则）实现区。

- `tiger/`：**虎句**（虎码字/词/句）——当前唯一全量实现；物理拆分在 P4（需先定义 `hux_core::scheme` 契约，
  否则 core 与方案会互相依赖）；
- `yuhao/`、`wubi/`：形码族骨架（预期复用虎句的解码 / 词先验 / 学习框架）；
- `shuangpin/`、`quanpin/`：拼音族骨架（音节切分 + 拼音词典，接口预留）。
- 依赖方向：`hux-scheme/* → hux-core`；内核不依赖任何方案。
