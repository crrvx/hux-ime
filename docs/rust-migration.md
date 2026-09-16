# Rust 直迁设计（K0–K4）

> 2026-09-16 ｜ 关联：[`rime-semantics.md`](rime-semantics.md)、[`spike-report.md`](spike-report.md)

目标：虎整句迁移为 **fcitx5 原生 Rust 实现**；核心逻辑全量移植，Lua 仅作测试 oracle，无 librime 依赖。

## 1. 路线

| 阶段 | 内容 | 验收 |
|---|---|---|
| **K0** ✅ | spike：`cache` + `ngram` 移植 + 差分工具链 + 陷阱审计 | fixture 29,617 条、真实模型 62,777 条逐位一致（见 [`spike-report.md`](spike-report.md)） |
| **K1**（当前） | 计算核：lexicon、decode/beam、early-evidence、learning | 快照差分全绿 |
| **K2** | 交互引擎：buffer/caret、menu、键位 `repr`、标点、ascii_composer、Tab 锁/提前上屏、选项 | 键序列金样一致 |
| **K3** | fcitx5 addon：注册、候选/预编辑/上屏、状态菜单、配置、数据路径、LevelDb | 真机可用 |
| **K4** | 验收与打包 | 真机清单 + 性能/内存 |

移植纪律：计算部分机械翻译（逐位保真）；交互部分按行为契约自由设计。每个模块迁完即接线，差分常绿。

## 2. 仓库结构

```
Cargo.toml                     # workspace
crates/
  tigerclaw-core/              # 纯逻辑，无 fcitx5 依赖
    src/cache.rs  ngram.rs     # K0 ✅
    src/lexicon.rs decode.rs learning.rs    # K1
    src/key.rs session.rs punct.rs ascii.rs config.rs   # K2
  tigerclaw-addon/             # K3：唯一依赖 fcitx5 的 crate
goldens/                       # 差分金样（fixture 入库；真实模型抽样本地）
tools/                         # 金样生成/基准（Lua 参照侧）
docs/
```

依赖方向：`addon → core`；core 不依赖 fcitx5、不依赖 Lua。

## 3. 模块映射（Lua → Rust）

| Lua（源仓库 `lua/`） | 行数 | Rust | 阶段 | 差分手段 |
|---|---:|---|---|---|
| `tiger_sentence_cache.lua` | 40 | `cache.rs` | K0 ✅ | fixture 金样（缓存状态/淘汰序） |
| `tiger_sentence_ngram.lua` | 550 | `ngram.rs` | K0 ✅ | 逐位 logp/observed + cache_status |
| `tiger_sentence.lua`（词库/解码/证据） | ~2600 | `lexicon.rs` + `decode.rs` | K1 | 快照（同输入→同候选/分数位模式） |
| `tiger_sentence_learning.lua` | 435 | `learning.rs` | K1 | 现成 23k 检查重放 |
| `tiger_sentence.lua`（processor/translator/filter/ascii/options） | ~1250 | `key.rs` + `session.rs` + `punct.rs` + `ascii.rs` + `config.rs` | K2 | 键序列金样 |
| `tiger_sentence_ngram.lua`（TCSKNM01 legacy） | — | `ngram.rs` | K1 | 同上（快照） |

> `try_load`/`candidate_paths`（模型路径探测）随 K3 数据路径一并实现；TCSKNM01 legacy 随 K1。

## 4. 数据与目录

- 用户目录 `~/.local/share/fcitx5/tigerclaw`；共享目录 `/usr/share/fcitx5/tigerclaw`。
- 码表（`tiger_sentence.*.txt`）、`models/sentence-ngram-mobile.bin`、`symbols.yaml`、
  PY_c 转换产物（R2）、`tiger_sentence.options.yaml`、学习库 `<hash>.userdb/`（LevelDB 同构）。

## 5. fcitx5 集成要点（K3）

- addon 注册（`Category=InputMethod`、`OnDemand`）+ 输入法条目 conf；`InputMethodEngine` 实现。
- 会话：每个 `InputContext` 一份 core 会话；`reset/activate/deactivate` 对齐。
- UI 同步：按键后状态快照（preedit/候选/上屏）；preedit 光标做字节→字符换算。
- 状态菜单：4 个核心开关（提前上屏、单字重码组句、提前上屏至编码、全角/半角标点）。

## 6. 测试

1. **Lua 回归**（oracle）：`tools/run_regressions.py` 全绿；
2. **Rust 差分**：模块对金样逐位断言（fixture 入库；真实模型本地/定期）；
3. **键序列金样**（K2）：真 librime 探针生成「键序列→提交/候选/预编辑」，Rust 重放比对。

## 7. 风险

| 风险 | 缓解 |
|---|---|
| 浮点位级差异 | 位模式比较；K0 已实证 libm 一致 |
| `pairs` 遍历序 / `table.sort` 非全序 | K1 纪律：显式排序 + 全序 tie-breaker（见 spike 报告 §3） |
| 交互语义偏差 | 键序列金样 + 真机清单 |
| 真实模型未入库 | fixture 全量入库 + 真实模型本地/定期差分 |
| 反查词典转换质量 | R2 转换器 + rime 侧金样对照（K3） |
