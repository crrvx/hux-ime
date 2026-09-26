<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# crates/hux-scheme：输入方案区

输入方案（码表 + 解码 + 学习规则）实现区；依赖方向 `hux-scheme/* → hux-core`——内核不依赖任何方案， \
平台只在装配处构造。

## 现状

| 目录 | 方案 | 状态 | 说明 |
| --- | --- | --- | --- |
| `tiger/` | 虎句（`tiger_sentence`） | **已落地**为独立 crate `hux-scheme-tiger` | 虎码字 / 词 / 句的整句输入，唯一全量实现（详见下节） |
| `yuhao/` | 宇浩 | 骨架，未开工 | 形码族 |
| `wubi/` | 五笔 | 骨架，未开工 | 形码族；差异在码表 / 选重与排序参数 |
| `shuangpin/` | 双拼 | 骨架，未开工 | 拼音族 |
| `quanpin/` | 全拼 | 骨架，未开工 | 拼音族；差异在拼写解析与键位映射 |

## tiger（虎句，唯一全量实现）

- 数据与计算：`lexicon`、`decode`（beam 解码与早提交证据）、`lexical`（TCSLEX01 词先验）、 \
  `ngram`（TCSKNM02 模型）；反查 `sound_to_char_shape` / `char_to_sound_shape`。
- 交互与契约：`interaction`（处理器管线、锁与瞬态状态、早提交、学习粘合、 \
  `host::CommitObserver` 实现）；契约实现是 `scheme.rs` 的 `TigerScheme` \
  ——承载共享资源与全部会话状态，平台只经 `hux_core::scheme::Scheme` 驱动（装配处构造）。
- 差分测试在 `tests/`（`cargo test --workspace`）；语义、数据与金样参照 \
  [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)， \
  方案标识保持 `tiger_sentence`；活规则见 [`design.md`](../../docs/design.md)， \
  历史见 [`review-ledger.md`](../../docs/review-ledger.md)。

## 骨架方案的公共前置

- **族共性**：形码族（`yuhao/`、`wubi/`）预期复用虎句的 beam 解码 / 词先验 / 早提交 / 学习框架； \
  拼音族为音节切分 + 拼音词典 + 候选排序（独立 translator），仅共享会话 / 宿主链 / 学习机制。
- **数据需求**：

  | 目录 | 数据需求 |
  | --- | --- |
  | `yuhao/` | 码表与字频（同虎句格式：<br>`codes` / `char_ranks` / `full_code_whitelist` / `supplement`）、<br>码长与选重规则、<br>方案标识 |
  | `wubi/` | 86 / 98 版码表与字频（同虎句四件套格式）、码长与选重规则、方案标识 |
  | `shuangpin/` | 拼音词典与音节表（可用 `tools/generators/gen_pinyin_index.py` <br>从 `PY_c.dict.yaml` 生成同一格式的索引）、<br>双拼键位映射表、<br>可选词先验（`data/tiger_sentence.lexical.bin` 同格式） |
  | `quanpin/` | 拼音词典与音节表（同双拼）、<br>简拼 / 纠错规则表、<br>可选词先验与 n-gram 模型（TCSKNM02，模型不随包） |

- **契约需求**（`hux_core::scheme`）：实现 `Scheme`（方法清单见 \
  [`design.md`](../../docs/design.md) §2「落地形态」）——本区自报 `id` / \
  `option_declarations()`（4 个角色 `SCHEME_OPTION_ROLES`，键自持）/ \
  `learning_mode()`（据配置袋自算的不透明 mode 串）/ `apply_config`（逐角色回报诊断）/ \
  `host_options`；资产目录由平台解析后传入。
- **依赖方向与守卫**：`hux-scheme/<方案> → hux-core`（内核零方案依赖， \
  CI 守卫见 [`design.md`](../../docs/design.md) §4）；`hux-cfg` 只提供角色词汇与设置， \
  装配由平台构造，方案不反向依赖二者。
