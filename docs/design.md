<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 设计

hux-ime（虎虚）：虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。
参照实现（测试 oracle，仅开发/CI 使用）：<https://github.com/lvyww/tiger-sentense-rime>；
金样清单见 [`../goldens/README.md`](../goldens/README.md)，重新生成命令与校验和见
[`../goldens/regenerate.md`](../goldens/regenerate.md)。

## 1. 仓库结构

crate / 模块级结构与「结构正义」硬规则见 [`refactor.md`](refactor.md) §1 / §2（**单一来源**，此处不重复）；
参照实现 → Rust 的模块映射见 §2，fcitx5 侧集成要点见 §4。

依赖方向：`platform/* → hux-ffi / hux-cfg / hux-core`，`hux-scheme/* → hux-core`；
内核不依赖方案与平台、不依赖 fcitx5、不依赖 Lua。

- **确定性**：凡排序必带全序 tie-breaker；凡 `pairs` 影响可观测结果处显式排序；凡时间 / 随机全部注入。

## 2. 模块映射（参照 → Rust）

| 参照 | Rust | 差分手段 |
|---|---|---|
| `lua/tiger_sentence_cache.lua` | `hux-core`: `cache.rs` | fixture 金样（状态/淘汰序） |
| `lua/tiger_sentence_ngram.lua` | `tiger/ngram.rs` | `logp`/`obs`/`status` 逐位 |
| `lua/tiger_sentence.lua`（词库/解码/证据） | `tiger/lexicon.rs` + `tiger/decode.rs` | 数据索引 + 解码/证据/学习快照 |
| `lua/tiger_sentence_learning.lua` | `hux-core`: `learning.rs`（机制）+ `tiger/interaction/learning_glue.rs`（策略） | 检查重放 + learning 金样 |
| `lua/tiger_sentence_lexical.lua` | `tiger/lexical.rs`（TCSLEX01） | 词先验金样 |
| `lua/tiger_sentence.lua`（processor/translator/filter/选项） | `hux-core`: `key.rs` + `session.rs`；`tiger`: `interaction.rs`（+ `interaction/`） | 键序列金样 |
| librime `key_event`/`key_table` | `hux-core`: `key.rs` + `key_table.rs`（由源码生成） | 键金样（真 librime 探针） |
| librime `reverse_lookup_translator` | `tiger/sound_to_char_shape.rs`（TCSRV01） | 音反查金样 |
| librime 宿主链 | `hux-core`: `host.rs` + `punct.rs`（提交点回调见 `CommitObserver`） | 键序列金样 |

## 3. 数据与目录

- 目录解析在平台层（`platform/fcitx5/src/paths.rs`；内核不读环境变量）：
  只读目录 `HUX_DATA_DIRS`（覆盖）> `$XDG_DATA_HOME/fcitx5/hux`（缺省 `~/.local/share/fcitx5/hux`）
  > `$XDG_DATA_DIRS/*/fcitx5/hux`（缺省 `/usr/local/share`、`/usr/share`）；
  可写数据（选项 / 学习库 / 模型）落用户目录；开发可用 `HUX_DATA_DIRS`（冒号分隔）
  与 `HUX_MODEL` 覆盖。
- 运行数据：码表四件套（`tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`）、
  `models/sentence-ngram-mobile.bin`（TCSKNM02）、`symbols.yaml`、词先验（TCSLEX01）、音反查索引（TCSRV01）、
  `tiger_sentence.options.yaml`、学习库 `tiger_sentence_learning_<hash>.userdb/`（LevelDB 同构）。
- 仓库 `data/` 的清单、来源与署名见 [`../data/README.md`](../data/README.md) 与
  [`LEXICAL_PRIOR_ATTRIBUTION.md`](LEXICAL_PRIOR_ATTRIBUTION.md)。

## 4. fcitx5 集成要点

- **注册与构建**：`Category=InputMethod` + `OnDemand` + 输入法条目 conf；C++ 薄壳链接 Rust 静态库，
  构建/安装与依赖见 [`usage.md`](usage.md)。产物：`/usr/lib/fcitx5/libhux.so`、
  `/usr/share/fcitx5/{addon,inputmethod}/hux.conf`。
- **会话**：每输入上下文一个（fcitx5 `InputContextProperty`；组合/候选/学习暂存隔离，选项为引擎级
  并同步到全部会话）；失焦时由 fcitx5 核心/前端以预编辑原文提交客户端预编辑（fcitx5 惯例，不保留组合）；
  切换输入法/重置由本层直接丢弃（不提交；上游默认在切换时提交候选/预编辑，本实现取丢弃）。组合重建由
  `interaction::CompositionBuilder` 按参照 `ConcreteEngine::Compose` 语义（`input[..caret]`、
  按公共前缀增量保留段、提交后旧段不复用）。
- **宿主语义**（core `host.rs`，`processor` 返回 Forward 后执行）：

  | 组件 | 行为要点 |
  |---|---|
  | `key_binder` | `Tab`→Down、`Shift+Tab`→Up（`when: has_menu`） |
  | `selector` | 菜单导航与翻页（`page_size`、上/下翻页键与翻页循环可由配置覆盖；默认 `-`/`[` → Page_Up、`=`/`]` → Page_Down，**两侧同前置**：菜单可见（且非 `ascii_mode`）即判翻页——用户决定 B，见下）、Home/End；候选排列由配置写入 `_vertical`。参照 `Selector::PreviousPage` 在首页也 `Highlight(0)`（`menu/page_down_cycle` 只作用于 `NextPage`），本实现按参照；参照另写 `paging` 标签，本仓随其唯一读取方一并删除 |
  | `navigator` | 字节光标移动；Ctrl/Shift+Left/Right 跳到段首/段尾（未做音节 spans 细分）；Home/End 到组合起点/末尾 |
  | `express_editor` | space 确认/提交、BackSpace 撤销编辑、Delete 删光标处、Return 提交原文、Escape 取消；可打印字符先提交组合再交宿主 |
  | `punctuator` | 单键可打印 ASCII 查 `symbols.yaml`；组合中提交「组合文本 + 标点」；`{pair}` 交替 |

  > **翻页键不被标点分支遮蔽（本仓有意偏离上游 `abad411`）**：上游方案处理器对「菜单可见 +
  > 可打印 ASCII 标点」先确认组合再把原键 Forward 给宿主，`selector`/`key_binder` 的
  > `-`/`=`/`[`/`]` 翻页绑定因此永远轮不到。本仓在标点分支入口先问与宿主**同一套**判据
  > `hux_core::host::paging_action(...)`：判为翻页的键落回宿主链，其余标点维持上游行为。
  > **代价**：菜单可见时这几个键打不出标点（`ascii_mode` 或无菜单时不成立）。
  > 依据与最小复现见 [`upstream-deviations.md`](upstream-deviations.md) ①；
  > 受影响金样用例按 `DEVIATIONS` 登记期望值（金样字节不动）。

- **英文模式不实现**（设计取舍）：英文输入交由 fcitx5 切换输入法；大写字母经 `char_handler` 直通（先提交组合）。
- **提交与按键顺序**：可打印字符的 `char_handler` 在核心语义为「提交组合 + 不消费」（同 librime）；宿主层
  （`platform/fcitx5`）据此消费该键并以 `forwardKey` 重发，保证客户端先收到提交、后收到按键
  （与 fcitx5 核心 `KeyEventOrderFix` 修法一致）。**例外**：布局转换键（如系统 colemak + 方案 `Layout=us`）
  **不自行转发**，交回核心在 `ReservedLast` 提交转换后的字符——否则客户端会按系统布局重新解释该键。
  机制是 C ABI 处置位 `HUX_KEY_FORWARD_AFTER_COMMIT`（`crates/hux-ffi/include/hux_abi.h`）+
  `platform/fcitx5/shell/hux.cpp` 用 fcitx5 自己的 `keyEvent.forward()` 判断是否重发。
- **UI 同步**：preedit 参照 librime `Composition::GetPreedit`——高亮候选的 `preedit`（正常段按词
  分码，如 `sh ks`；音反查段按音节，如 `` `zhong guo ``）优先，组合之后的原始输入原样接在其后
  （左右移动光标时保持分码，如 `` ab cd `` + 尾部 `ja` → `` ab cdja ``）；无高亮候选时回退
  「缓冲 + 原始输入」；光标为字节偏移。
- **反查**：音反查（`sound_to_char_shape.rs`）语义对齐 librime 词典反查——拼写缩写罚 `log 0.5`、全拼可达时
  缩写路径剪枝、补全罚 `log 0.05`、排序 = 可信度 + `ln(权重)`、上限 20；字反查（`char_to_sound_shape.rs`）
  取光标左侧 1 字，上排拼音（排头「咅」）、下排虎码（排头「虍」）。两者触发键可配置，**仅单字符触发键**
  给默认可上屏候选。详见 [`../platform/fcitx5/README.md`](../platform/fcitx5/README.md)。
- **候选点击**：面板候选为自定义 `CandidateWord`，点击经 `hux_engine_select_candidate` 按全局索引
  选中并上屏（与空格同一条确认/学习链）。
- **学习**：提交点通知器（参照 `Context::Commit` 的 `commit_notifier`）内建于核心路径
  （`confirm_selection`、自动上屏）与宿主链提交点（`editor` char_handler、`punctuator`）；
  候选点击经确认链记录；宿主排空 `LiveLearning::submitted` 落库并在 `store_ready` 后生效；
  存储 `<user>/tiger_sentence_learning_<hash(schema_id)>.userdb/`（1 万条 / 16 MiB，60 秒节流刷新）。
- **选项键归属**：方案经 `Scheme::option_declarations` 自报「角色 → 键」，角色词汇与默认值在
  `hux-cfg::roles`（宿主标准项 `full_shape`/`ascii_punct` 由配置层自持）；平台装配处解析成角色表，
  **缺角色即报错**（状态串可见）——宿主与配置层都不硬编码方案选项名。
- **选项与配置**：`tiger_sentence.options.yaml`（主）+ legacy `user.yaml` 的 `var/option/*`（只读回退，
  保存失败写属性 `tiger_sentence_options_error`）；合并顺序 **options.yaml > 设置 > 内建缺省**，
  但配置页推送时设置值**写回** `options.yaml`（两侧是同一批项，故互不压制）；图形配置由
  C++ `HuxConfig` schema 生成（「行为」「快捷键」两区，子配置 + `ToolTipAnnotation`；快捷键为 `KeyList`），
  经 `hux_engine_apply_settings` 应用；状态菜单（「虎虚」子菜单，5 项核心开关：提前上屏、提前上屏至预编辑、
  单字重码组句、全角标点、数字直选）经 `hux_engine_set_option` 切换并写入 `options.yaml`，
  宿主同时镜像进 `conf/hux.conf`——任一侧改动即时生效且另一侧读到同一值。

## 5. 测试

1. **Rust 差分**：模块对金样逐位断言（fixture 入库；真实模型本地/定期）；
2. **键序列金样**：真 librime 探针生成「键序列 → 提交/候选/预编辑」，Rust 重放比对；
3. **CI**：fmt/clippy/差分 + 以固定参照提交重生成 fixture 金样比对（溯源校验），见
   [`../goldens/regenerate.md`](../goldens/regenerate.md)。
