<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# crates/hux-scheme

输入方案（码表 + 解码 + 学习规则）实现区。

- `tiger/`：**虎句**（虎码字/词/句）——当前唯一全量实现，**已落地**为独立 crate
  （`hux-scheme-tiger`：码表/解码/词先验/模型/反查 + 交互策略）；
- `yuhao/`、`wubi/`：形码族骨架（预期复用虎句的解码 / 词先验 / 学习框架）；
- `shuangpin/`、`quanpin/`：拼音族骨架（音节切分 + 拼音词典，接口预留）。
- 依赖方向：`hux-scheme/* → hux-core`；内核不依赖任何方案。
