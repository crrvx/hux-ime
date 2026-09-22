<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# crates/hux-scheme/tiger

**虎句**（`tiger_sentence`）方案：虎码字 / 词 / 句的整句输入（当前唯一全量实现，P4b 落地）。

- 数据与计算：`lexicon`（码表 / 字频 / 白名单 / 补充）、`decode`（beam 解码与早提交证据）、
  `lexical`（TCSLEX01 词先验）、`ngram`（TCSKNM02 模型）；
- 反查：`sound_to_char_shape`（音反查）、`char_to_sound_shape`（字反查）；
- 交互策略：`interaction`（处理器管线、锁与瞬态状态、早提交、学习粘合、`host::CommitObserver` 实现）；
- 契约实现：`scheme.rs` 的 `TigerScheme`——承载共享资源（解码器 / 标点表）与全部会话状态，
  实现 `hux_core::scheme::Scheme`；平台只经契约驱动（装配处构造 `TigerScheme`）；
- 差分测试在 `tests/`（`cargo test --workspace`；本地真实模型抽样缺失自动跳过）；
- 依赖方向：`hux-scheme/* → hux-core`；内核不依赖本 crate，平台仅在装配处构造方案；
- 语义、数据与金样参照 [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)，
  方案标识保持 `tiger_sentence`。活规则（结构 / 契约）见 [`../../../docs/refactor.md`](../../../docs/refactor.md)，批次与复核整改记录见
  [`../../../docs/review-ledger.md`](../../../docs/review-ledger.md)。
