# AGENTS.md

hux-ime：虎句（`tiger_sentence`）输入方案从 fcitx5-rime 迁移到 fcitx5 原生实现。

## AI 风格
1. 中文回答，简明扼要；
2. 若有任何未尽事宜，随时提问/建议；
3. 提出问题时，同时列举可行的方案供用户选择。

## 任务
1. fcitx5-rime -> fcitx5 原生（Rust 直迁）；
2. 核心逻辑全量移植 Rust（Lua 退居测试 oracle，不进运行时）；
3. 以差分验证保证行为等价（fixture 金样入库；真实模型本地差分）。

## 原则
1. 自顶向下设计，自底向上实现；
2. 模块化-高内聚低耦合；
3. 代码简练但清晰；
4. 文档详略得当且内容完备。

## 协作流程
1. AI 的每次修改：若当前节点非空，则在对应历史节点上 `jj new` 开新副本，改动只落在该副本内，不直接改动已有节点；
2. 修改完成后交由用户审阅确认；
3. squash 与历史整理由用户自行执行，AI 不代做。

## 命名
1. 讲我们：`hux-ime`（简写 `hux`）——crate、addon ID、数据目录、环境变量等工程标识一律用它；
2. 讲方案/数据/标识：`tiger_sentence`（显示名「虎句」）——数据文件、选项、学习库与金样沿用该标识（与上游 rime 方案互通），保持不变；
3. 讲上游：`虎爪` = [tigerclaw](https://github.com/lvyww/tigerclaw)（原生），`虎整句`／[tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)（rime 版，即「虎爪-rime」）。

## 背景与约定
- 参考实现（Rime 方案 + Lua 核心）：<https://github.com/lvyww/tiger-sentense-rime>
  - 金样生成需本地检出：外部检出统一放仓库内 `external/`（已 gitignore），默认
    `external/tiger-sentense-rime`（`git clone https://github.com/lvyww/tiger-sentense-rime external/tiger-sentense-rime`）；
    可用 `HUX_REFERENCE_REPO` / `--reference` / `REF` 覆盖；线上地址即上
  - 主引擎 `lua/tiger_sentence.lua`；学习 `tiger_sentence_learning.lua`；
    n-gram `tiger_sentence_ngram.lua`；缓存 `tiger_sentence_cache.lua`
  - 测试 `tools/test_*.lua` + `tools/run_regressions.py`：迁移期作为逐位等价 oracle
- 路线：K0（已完成）→ K1 计算核 → K2 交互引擎 → K3 fcitx5 addon → K4 验收；
- 移植纪律：计算部分机械翻译 + 差分逐位验证；交互部分按行为契约自由设计；
- Lua 仅作测试 oracle（CI/开发环境），不进入运行时依赖；
- 版本控制：jj（Jujutsu）colocate 模式；日常操作走 jj，不直接使用 git；
- 文档：`docs/rust-migration.md`（设计）、`crates/hux-addon/README.md`（addon 使用）、`goldens/README.md`（金样）、
  `data/README.md`（数据）、`docs/LEXICAL_PRIOR_ATTRIBUTION.md`（词先验署名/许可）。
