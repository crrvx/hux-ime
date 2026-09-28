<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 参考：模块映射与数据布局

- 查表式参考（从 [`design.md`](design.md) 拆出，规则叙述见该文）
  - §1 参照实现 → Rust 的模块映射（含各模块差分手段）
  - §2 数据与目录的解析顺序

## 1. 模块映射（参照 → Rust）

| 参照 | Rust | 差分手段 |
| --- | --- | --- |
| `lua/tiger_sentence_cache.lua` | `hux-core`: `cache.rs` | fixture<br>金样（状态/淘汰序） |
| `lua/tiger_sentence_ngram.lua` | `tiger/ngram.rs` | `logp`/`obs`/`status`<br>逐位 |
| `lua/tiger_sentence.lua`<br>（词库/解码/证据） | `tiger/lexicon.rs`<br>+ `tiger/decode.rs` | 数据索引 + 解码/证据/学习快照 |
| `lua/tiger_sentence_learning.lua` | `hux-core`: `learning.rs`（机制）<br>+ `tiger/interaction/learning_glue.rs`（策略） | 检查重放 + learning 金样 |
| `lua/tiger_sentence_lexical.lua` | `tiger/lexical.rs`<br>（TCSLEX01） | 词先验金样 |
| `lua/tiger_sentence.lua`<br>（processor/translator/filter/选项） | `hux-core`: `key.rs` + `session.rs`；<br>`tiger`: `interaction.rs`（+ `interaction/`） | 键序列金样 |
| librime `key_event`/`key_table` | `hux-core`: `key.rs`<br>+ `key_table.rs`（由源码生成） | 键金样（真 librime 探针） |
| librime `reverse_lookup_translator` | `tiger/sound_to_char_shape.rs`<br>（TCSRV01） | 音反查金样 |
| librime 宿主链 | `hux-core`: `host.rs` + `punct.rs`<br>（提交点回调见 `CommitObserver`） | 键序列金样 |

## 2. 数据与目录

- 目录解析在平台层（内核不读环境变量）：
  - 根规则在落点（XDG）：`platform/linux/src/lib.rs`
  - 根规则在落点（宿主注入）：`platform/android/src/lib.rs`
  - 拼接 `fcitx5/hux`、顺序与 `HUX_DATA_DIRS`：`platform/fcitx5/src/paths.rs`
- 只读目录查找顺序：
  - `HUX_DATA_DIRS`（覆盖，冒号分隔）
  - `$XDG_DATA_HOME/fcitx5/hux`，缺省 `~/.local/share/fcitx5/hux`
  - `$XDG_DATA_DIRS/*/fcitx5/hux`，缺省 `/usr/local/share`、`/usr/share`
  - 末级 `/usr/share/fcitx5/hux`
- 开发可用 `HUX_DATA_DIRS`（冒号分隔）与 `HUX_MODEL` 覆盖
- 安装去向见 [`resources.md`](resources.md)「落点与查找顺序」（§0.2）
- 运行数据：码表四件套 + 追加码表、模型、`symbols.yaml`，另有词先验、音反查索引、选项与学习库
  - 文件名 / 格式 / 来源见 [`../data/README.md`](../data/README.md)
  - 一并见 [`resources.md`](resources.md)
