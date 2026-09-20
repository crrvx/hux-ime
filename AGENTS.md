<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# AGENTS.md

虎虚-输入引擎（hux-ime），rust 实现

## 项目定位

- **虎虚 hux**：fcitx5 原生输入引擎（Rust 实现），内核独立于方案与平台
- 平台优先级：linux / android → windows → macos / ios
- 运行时不依赖 librime、不依赖 Lua（Lua 仅作测试 oracle）
- 结构与重构原则见 `docs/refactor.md`（活规则）；历史与逐批记录见 `docs/review-ledger.md`，有意偏离见 `docs/upstream-deviations.md`

## 当前方案

- **虎句**（`tiger_sentence`）：虎码体系（字 / 词 / 句）的整句输入方案，本轮唯一全量实现
- 语义、数据与金样参照 tiger-sentense-rime；方案标识保持 `tiger_sentence`（与上游数据互通）
- 其他方案（宇浩、五笔、双拼、全拼）仅留骨架，适配暂缓（见 `docs/refactor.md`）

## AI 风格

1. 中文回答，简明扼要
2. 若有任何未尽事宜，随时提问/建议
3. 提出问题时，同时列举可行的方案供用户选择

## 原则

1. 自顶向下设计，自底向上实现
2. 模块化：高内聚，低耦合
3. 文档：详略得当，内容完备
4. 代码：简练，清晰
5. 测试：单元测试，集成测试

## 协作流程

1. 修改开始前：在当前所在节点上，先执行 `jj new` 开新副本，再进行改动
2. 修改过程中：可按需执行更多的 `jj new`，只落在 ai `jj new` 的副本内，不改动其他已有节点
3. 修改完成后：须同步对应文档，根据改动编辑 “jj commit” 消息，并交由用户审阅确认
4. jj 历史整理, push/fetch：由用户自行执行，AI 不代做

## 相关项目

1. 上游项目

- `虎爪` [tigerclaw](https://github.com/lvyww/tigerclaw)（win 原生）
- `虎整句` [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)（即「虎爪-rime」）

2. 参考实现

- [tigirl](https://github.com/lvyww/tigirl)
- [tigerclaw](https://github.com/lvyww/tigerclaw)
- [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)
- [hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust)

## 背景与约定

- 命名约定：
  - 项目/引擎/内核：中文「虎虚」，英文「hux」
  - 方案/数据/标识：「虎句」，tiger_sentence 等
- Rust 约定：
  - 模块布局用 `foo.rs` + `foo/`，**切忌 `mod.rs`**
  - 集成测试的共享助手 `crates/hux-test-support`，请以 `dev-dependencies` 引入（不得使用 `tests/common/mod.rs`）
- 参考实现：
  - tiger-sentense-rime 的 Lua 核心，仅作测试 oracle（不进运行时）
  - 检出放仓库内 `_external/tiger-sentense-rime`（.gitignore，`HUX_REFERENCE_REPO` / `REF` 可覆盖）
- 移植纪律：
  - 计算部分机械翻译 + 差分逐位验证，交互部分按行为契约设计
  - 重构 / 优化不得改变可观测行为
- 文档：
  - 活文档只写现状（索引见根 `README.md`「文档」表）
  - 历史与逐批记录进 `docs/review-ledger.md`
  - 有意偏离进 `docs/upstream-deviations.md`

## 特殊目录
- `_tmp/`：本地开发 / 临时记录
- `_external/`：用于快捷访问 本地外部文件
