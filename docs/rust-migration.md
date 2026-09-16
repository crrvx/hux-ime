# Rust 直迁设计（K0–K4）

> 2026-09-16 ｜ 关联：[`rime-semantics.md`](rime-semantics.md)、[`spike-report.md`](spike-report.md)

目标：虎整句迁移为 **fcitx5 原生 Rust 实现**；核心逻辑全量移植，Lua 仅作测试 oracle，无 librime 依赖。

## 1. 路线

| 阶段 | 内容 | 验收 |
|---|---|---|
| **K0** ✅ | spike：`cache` + `ngram` 移植 + 差分工具链 + 陷阱审计 | fixture 29,617 条、真实模型 62,777 条逐位一致（见 [`spike-report.md`](spike-report.md)） |
| **K1** ✅ | 计算核：lexicon、decode/beam、early-evidence、learning（见 [`spike-report.md`](spike-report.md) 与金样） | 快照差分全绿 |
| **K1.5** ✅ | 上游追平：紧凑排序先验（码形证据 / 4 码生僻字保护 / Top-5 词先验）+ 锁播种修复语义；pin 前移至上游 main `35a10b9`，金样全量重生成 | 模型版金样逐位一致（含词先验重排） |
| **K2** ✅ | 交互引擎：buffer/caret、menu、键位 `repr` ✅（`key.rs` + 键表生成/金样）、键序列金样 ✅（2c 探针 61 例/273 步，含空码自动上屏、编辑/导航键、ascii Shift 切换、标点表、大写字母）、处理器/翻译器/过滤器/学习暂存与提交通知器/早提交 ✅；宿主等价物见 K3 ⑥ | 键序列金样一致 |
| **K3**（进行中） | fcitx5 addon：注册、候选/预编辑/上屏、数据路径、选项、学习库 ✅；宿主编辑语义 ✅（⑥）；ascii_composer ✅（⑦a）；标点表 ✅（⑦b）；反查、打包、状态菜单待做 | 真机可用 |
| **K4** | 验收与打包 | 真机清单 + 性能/内存 |

移植纪律：计算部分机械翻译（逐位保真）；交互部分按行为契约自由设计。每个模块迁完即接线，差分常绿。

## 2. 仓库结构

```
Cargo.toml                     # workspace
crates/
  tigerclaw-core/              # 纯逻辑，无 fcitx5 依赖
    src/cache.rs  ngram.rs                        # K0 ✅
    src/lexicon.rs decode.rs learning.rs          # K1 ✅
    src/lexical.rs                                # K1.5（紧凑词先验）
    src/key.rs key_table.rs session.rs interaction.rs   # K2（key 事件/会话/交互）
  tigerclaw-addon/             # K3：C++ 薄壳（shell/）+ Rust FFI（src/）→ core
data/                          # 随包数据源（词先验位图，CC BY 4.0）
goldens/                       # 差分金样（fixture 入库；真实模型抽样本地）
tools/                         # 金样生成/基准（Lua 参照侧、真 librime 探针）
docs/
```

依赖方向：`addon → core`；core 不依赖 fcitx5、不依赖 Lua。

## 3. 模块映射（Lua → Rust）

| Lua（源仓库 `lua/`） | 行数 | Rust | 阶段 | 差分手段 |
|---|---:|---|---|---|
| `tiger_sentence_cache.lua` | 40 | `cache.rs` | K0 ✅ | fixture 金样（缓存状态/淘汰序） |
| `tiger_sentence_ngram.lua` | 550 | `ngram.rs` | K0 ✅ | 逐位 logp/observed + cache_status |
| `tiger_sentence.lua`（词库/解码/证据） | ~2600 | `lexicon.rs` ✅ + `decode.rs` ✅（冷路径 + 证据 + 学习接线） | K1 | 数据索引金样 + 解码/证据/学习快照；增量/锁缓存未移植（性能项） |
| `tiger_sentence_learning.lua` | 435 | `learning.rs` ✅ | K1 | 23k 检查重放 + learning 金样 |
| `tiger_sentence_lexical.lua` | 152 | `lexical.rs` ✅（TCSLEX01） | K1.5 | 词先验金样（读取/Bloom/打分；真实位图） |
| `tiger_sentence.lua`（processor/translator/filter/ascii/options） | ~1250 | `key.rs` ✅ + `session.rs` ✅ + `interaction.rs` ✅（会话运行时与交互层） | K2 | 键序列金样 |
| librime `key_event`/`key_table`（宿主行为） | — | `key.rs` + `key_table.rs` ✅（由源码生成） | K2 | librime 探针金样 |

> `try_load`/`candidate_paths`（模型路径探测）随 K3 数据路径一并实现。

## 4. 数据与目录

- 用户目录 `~/.local/share/fcitx5/tigerclaw`；共享目录 `/usr/share/fcitx5/tigerclaw`。
- 码表（`tiger_sentence.*.txt`）、`models/sentence-ngram-mobile.bin`、`symbols.yaml`、
  PY_c 转换产物（R2）、`tiger_sentence.options.yaml`、学习库 `<hash>.userdb/`（LevelDB 同构）。
- 仓库内 `data/` 为随包数据源：`symbols.yaml`（标点表；half_shape 的 `/` 提交 `/`，参照原表为 `、`）、
  `tiger_sentence.lexical.bin`（紧凑词先验，TCSLEX01
  Bloom filter；CC BY 4.0 署名见 `docs/LEXICAL_PRIOR_ATTRIBUTION.md`，参数与校验和见
  `docs/LEXICAL_PRIOR_MANIFEST.json`，CI 按 sha256 校验）。

## 5. fcitx5 集成要点（K3）

- addon 注册（`Category=InputMethod`、`OnDemand`）+ 输入法条目 conf；`InputMethodEngine` 实现。
- 构建/安装：`cmake -S crates/tigerclaw-addon -B build/addon -DCMAKE_INSTALL_PREFIX=/usr`
  → `cmake --build` → `cmake --install`；产物 `/usr/lib/fcitx5/libtigerclaw.so` 与
  `/usr/share/fcitx5/{addon,inputmethod}/tigerclaw.conf`（C++ 薄壳链接 Rust 静态库）。
- 会话：每引擎单会话（`activate/deactivate/reset` 清空）；组合重建由
  `interaction::CompositionBuilder` 负责（参照 `ConcreteEngine::Compose`：分段输入随光标
  —— `input[..caret]`，caret 处无已确认段且不在末尾时翻译到 caret 后一段；按新旧输入公共
  前缀增量保留段，未变的段保留菜单与高亮；提交后旧段不复用）。
- 宿主处理器链：core `host::process_key` 在 `processor` 返回 `Forward` 后执行 librime
  原生组件等价物（`key_binder` → `selector` → `navigator` → `express_editor`；`speller`/
  `punctuator` 见 ⑦）：菜单导航/翻页、字节光标移动（Home/End、Ctrl/Shift+Left/Right）、
  退格/删除；`Consumed` 时宿主吞键，`Forward` 时交基础应用（空闲编辑键）。
- ascii_composer：core `ascii::AsciiComposer` 在链首（`processor` 之前）：Shift 轻击/Caps
  切换 `ascii_mode`（样式与缓冲态归一照参照），`Accepted` 吞键、`Rejected` 交宿主并停止链
  （ascii 空闲直通）、`Noop` 继续。
- UI 同步：按键后状态快照（preedit/候选/上屏）；preedit 光标为字节偏移
  （fcitx `Text::setCursor` 即字节制）。
- 数据：core `lexicon::data_directories()`（用户 → 共享）与 `candidate_paths()` 探测；
  addon 加载码表/位图/模型（开发可用 `TIGERCLAW_DATA_DIRS`/`TIGERCLAW_MODEL` 覆盖）。
- 学习：提交点的通知器序列（选择/暂存/提交）已内置在核心提交路径
  （`confirm_selection`、自动上屏的 `LearningCommit`）；宿主只需排空
  `LiveLearning::submitted` 落库，并在 `store_ready` 置位后生效；宿主自发的提交
  （如候选点击）调 `interaction::learning_commit`。存储为
  `<user>/tiger_sentence_learning_<hash(schema_id)>.userdb/`（LevelDB，1 万条/16 MiB，60 秒节流刷新）。
- 选项：`tiger_sentence.options.yaml`（主）+ legacy `user.yaml` 的 `var/option/*`（只读回退）；
  保存失败写属性 `tiger_sentence_options_error`（core `Options` 提供同步/抑制语义）。
- 配置：addon `Settings`（7 项：早提交三项、full_shape/ascii_punct、tab_learning、high_freq_limit）
  与合并顺序（options.yaml > 设置 > 内建缺省）；图形配置：C++ 壳声明 `TigerclawConfig` schema + `getConfig/setConfig`（fcitx5-configtool 生成设置页），经 ABI `tigerclaw_engine_apply_settings` → `Settings::apply_settings`。
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
