<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 设计

hux-ime（虎虚）：虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。
参照实现（测试 oracle，仅开发 / CI 使用）：<https://github.com/lvyww/tiger-sentense-rime>；
金样清单、重新生成命令与校验和见 [`goldens/README.md`](../goldens/README.md)。

本文是**活规则 + 设计现状**的单一来源。原重构文档 `docs/refactor.md` 已并入本文： \
其 §1 / §2 → 「结构与硬规则」、§5 → 「方案契约」、§6 → 「测试与性能纪律」、 \
§7 → 「依赖校验」、§9 → 「骨架」。历史与逐批记录（原 §3 / §4、上游追平、未闭合项、 \
审计总账）见 [`review-ledger.md`](review-ledger.md)；有意偏离（编号沿用原 §8： \
① 翻页 / 标点遮蔽修复（含用户决定 B）、② addon扩展、③ pin 差异、④ 宿主链交互； \
含金样策略与回归做法）见 [`upstream-deviations.md`](upstream-deviations.md)； \
文档纪律见 `AGENTS.md`。

## 1. 结构与硬规则

1. **依赖单向**：
  - `platform/*` 依赖 `hux-ffi` / `hux-cfg` / `hux-core`；
  - 对具体方案（`platform/* → hux-scheme/*`）只在**装配处**、经契约与装配面常量依赖， \
    不得引用方案内部模块；
  - `hux-cfg → hux-core`；
  - `hux-scheme/* → hux-core`；
  - 内核不依赖任何方案、不依赖任何平台。
2. **core 零平台**：
  - 不得出现 `std::env`、XDG / 绝对数据路径解析、`SystemTime::now`、`eprintln!`；
  - 路径 / 时钟 / 日志由平台构造并注入（目录列表 / `now: f64` / notes 汇总）。 \
    **允许**读取平台传入的**显式路径**（如 [`PunctTable::load`] 按给定路径读 `symbols.yaml`）—— \
    core 只做「给定路径 → 解析」，不解析环境、不拼接平台目录；
  - CI 的 platform-clean 检查据此只拦 env / XDG / 时钟 / 直接打印。
3. **职责归位**：
  - 只读数据（码表 / 模型 / 索引）属方案；
  - 可写数据（选项 / 学习库）属 `hux-cfg` 与平台存储实现；
  - UI 快照、C++ 壳、打包属 `platform`；
  - C ABI 是平台边界（`hux-ffi`）。
4. 模块名即职责；单元测试随模块、集成 / 差分测试独立目录（二者分离）。

**确定性**：凡排序必带全序 tie-breaker；凡 `pairs` 影响可观测结果处显式排序； \
凡时间 / 随机全部注入。

**目标结构**：

```
crates/                       # 平台无关的 Rust 库
  hux-core/                   # 引擎内核：key/session/composition/处理器管线/宿主链/学习机制
                              #   + 方案契约（hux_core::scheme）
  hux-cfg/                    # hux 自身可配置项：设置项定义与默认值、选项存储与合并顺序
                              #   （options.yaml > 设置 > 内建；配置页推送时设置值写回存储）、
                              #   状态菜单开关白名单、持久化接口
  hux-ffi/                    # C ABI：C 布局类型 + 导出函数（桌面 / Android 共用）
  hux-scheme/
    tiger/                    # 虎码（字/词/句）——当前唯一全量实现
    yuhao/  wubi/             # init：骨架（形码族，复用 tiger 框架；说明见 README）
    shuangpin/  quanpin/      # init：骨架（拼音族，接口预留；说明见 README）
  hux-test-support/           # 测试助手（金样路径 / transcript 编解码 / 临时目录）
platform/                     # 平台适配
  fcitx5/                     # 共享 fcitx5 适配：Rust 组装（Engine/UI 快照/存储实现/Paths）
                              #   + C++ 壳 + CMake（linux 与 android 共用）
  linux/                      # 桌面：构建 / 安装说明（入口脚本在仓库根；打包待做）
  android/                    # Android：构建接线（对接 fcitx5-android fork 的 plugin/hux）
  windows/  macos/  ios/      # init：骨架（说明见 platform/README.md）
```

平台层分工：`platform/fcitx5` 是**共用适配**（两端都是 fcitx5，环境变量与路径解析同一套）， \
`platform/linux` / `platform/android` 只管各自的**构建与分发**。文件级模块： \
`hux-core` 有 `cache` / `learning` / `key` / `key_table` / `session` / `host` / `punct` / \
`scheme`（方案契约），`hux-scheme/tiger` 有 `lexicon` / `decode` / `lexical` / `ngram` / \
`sound_to_char_shape` / `char_to_sound_shape` / `interaction`（+ `interaction/`）； \
`hux-cfg`（设置与默认值、选项存储与合并顺序）、`hux-ffi`（C 布局类型 + `include/hux_abi.h`）、 \
`hux-test-support`（dev 依赖：金样路径 / transcript 编解码 / 临时目录）； \
`platform/fcitx5` 是 C++ 薄壳（`shell/hux.cpp`）+ Rust 组装（`engine` / `session` / `ui` / \
`paths` / `learning_store` / `abi`，导出 C ABI）。

其余目录：`data/`（随包数据源）、`assets/branding/`（品牌图形， \
主源 `hux.png`）、`goldens/`（差分金样与夹具）、`tools/`（生成器 / 探针 / 用例）、 \
`docs/`（索引见根 `README.md`「文档」表）；`platform/android` 的插件接线**待启动**、 \
`platform/linux` 打包待做，见 [`../platform/README.md`](../platform/README.md)； \
参照实现 → Rust 的模块映射（含各模块差分手段）见本文「模块映射」。

> **现状**：`crates/hux-cfg`、`crates/hux-ffi`、`crates/hux-scheme/tiger`、`platform/fcitx5`、 \
> `platform/linux` 均已落地；`hux-core` 只余通用内核 \
> （cache/collections/key/key_table/learning/punct/session/host） \
> **+ 方案契约 `hux_core::scheme`**；平台装配根构造 tiger 后以 `dyn Scheme`驱动。

## 2. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案必然依赖 core 类型（`KeyEvent` / `Context` / `Candidate`…）； \
  单开 interface crate 无净收益，将来接口变大或需「方案作者 SDK」再拆（纯移动 + `pub use`兜底）。
- **最小契约**：
  - 只定义内核必须回调的动作（`id` / 选项声明 / 按键与翻译重建 / 学习策略 / 反查展示）；
  - **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约——先留 `tiger` profile， \
    等第二个同族方案落地再抽象。形码族（虎码 / 宇浩 / 五笔）优先；
  - 拼音族（双拼 / 全拼）只留接口。
- **选项键单一来源 + 角色归配置层**：方案经 `Scheme::option_declarations` 自报「角色 → \
  键」（`&'static [OptionDecl]`），**角色词汇与默认值归 `hux-cfg`**（`hux_cfg::roles` 常量； \
  宿主标准项 `full_shape` / `ascii_punct` 由配置层自持）。 \
  平台装配处解析为`OptionKeys`角色表（**缺角色即报错**，不静默接线）， \
  `Settings::{option_defaults, store_defaults, option_default}`、`options::option_defaults`、 \
  `OptionsStore::load` 与状态菜单白名单（角色序 = C ABI `HUX_OPTION_*` 序）均按该表工作。 \
  键的**持久化兼容**由方案测试 `option_declarations_are_stable_persisted_keys` 钉住， \
  「每个角色都必须被方案声明」由平台测试 `every_configured_role_is_declared_by_the_scheme`钉住， \
  YAML 读写由 `hux-cfg` store 测试（含历史键字面量）守护。
- **角色一致性守护**（记录见 [`review-ledger.md`](review-ledger.md)「历史纪要」）： \
  两份同值字面量（`hux-cfg` / 方案）+ `Config::parse` 对未知角色 `unwrap_or(0/false)` 静默回退 ⇒ \
  单侧改名可让 `min_retained_raw_length` / `high_freq_limit` 静默失效而全绿。故：
  - ①方案自报 `hux_scheme_tiger::scheme::SCHEME_CONFIG_ROLES`， \
    `TigerScheme::load` 报「未识别 / 缺少角色」诊断（进 `hux_engine_status`）；
  - ②平台测试 `scheme_config_roles_match_the_scheme`（清单逐项比对 + \
    真实装配路径无诊断+改名必报诊断）、 \
    `runtime_role_tables_cover_the_declared_roles`（`SCHEME_OPTION_ROLES` ⊆ \
    `RUNTIME_OPTION_ROLES`、存储 / 会话缺省覆盖运行时角色）、 \
    `option_role_order_matches_the_abi_header`（`hux_abi.h` 的 `HUX_OPTION_*` 枚举序 ↔ \
    `RUNTIME_OPTION_ROLES`）钉住全部四张清单；
  - ③`hux_abi.h` 的 `HUX_OPTION_COUNT` + C++ \
    `static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`把「加角色未补文案」从越界读（UB）变 \
    成编译失败。
- **口径命名**：配置 / ABI / 平台层的**标识符**描述引擎概念（`ROLE_MIN_RETAINED_INPUT_LENGTH`、 \
  `ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS`、`ROLE_REVERSE_LOOKUP_CHARACTER_KEYS`、 \
  `ROLE_LEARNING_ON_TAB` 及对应 `Settings` 字段 / `hux_options` 成员 / C++ 配置成员）； \
  **线上字符串一律不动**——`ROLE_*` 的值仍是与上游 schema / \
  rime 选项同名的键（`"min_retained_raw_length"` / `"sound_to_char_shape_keys"` / \
  `"char_to_sound_shape_keys"` / `"tab_learning"`），`shell/hux.cpp` 的 `.path{}` / \
  schema 默认值路径、`tiger_sentence_*` 前缀、学习库目录名、`options.yaml` 与 `Library` / \
  `Icon` 保持原样。方案侧的**模块 / 函数名**（`sound_to_char_shape` / \
  `char_to_sound_shape` 及内部 helper）是参照移植的溯源名。
- **方案配置袋**：`SchemeConfig` 是「角色 → `Value`（开关 / \
  计数 /文本 /文本列表）」的**通用键值袋**—— \
  平台按角色装配（全集`hux_cfg::roles::SCHEME_CONFIG_ROLES`， \
  完整性由平台测试 `scheme_config_covers_every_declared_role`守护），方案按角色解释； \
  内核不再有 `min_retained_raw_length` / 反查键 / Tab 学习等虎码口径字段，换方案不必改 core。
- **内核不 import 任何 `hux-scheme/*`**（校验见本文「依赖校验」）。
- **配置诊断通道**（记录见 [`review-ledger.md`](review-ledger.md)「历史纪要」）： \
  `Scheme::apply_config(&SchemeConfig) -> Result<(), Vec<ConfigError>>`—— \
  `SchemeConfig::require_{bool,count,text,texts}`区分「角色缺失」与「类型不符」（`ConfigError`带 \
  角色名与期望类型），方案按缺省回退并回诊断，平台并入状态串（`config:` 前缀）； \
  `texts` 改为 `Option<&[String]>`。
- **落地形态**：`hux_core::scheme::Scheme` 只含「必须回调方案」的动作——`id` / \
  `option_declarations` / `learning_mode` / `apply_config` / `host_options` / \
  `set_store_ready` / `apply_learning_index` / `new_session` / `free_session` / \
  `reset_session` / `process_key` / `select_candidate` / `rebuild` / `take_learning_events` / \
  `buffered_text` / `auxiliary_lookup_active` / `auxiliary_rows`； \
  学习 mode 由方案据配置袋**自算**（平台只取不透明串 `Scheme::learning_mode`）， \
  虎码特有语义（缓冲态、锁、早提交启发式、证据、mode 串格式）全留 `TigerScheme`。

## 3. 测试与性能纪律

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle。现状： \
  含内联单测的源文件 **25 个**（内核 8 / 方案 8 / 配置 4 / 助手 1 / 平台 3 / ffi 1）， \
  集成与差分 7 个在 `tests/`（内核 2 / 方案 5）—— \
  两者不混放（数法`grep -rl '#\[cfg(test)\]' crates platform`）。
- `hux-test-support`：
  - 只放与业务无关的共性工具——金样 / 夹具路径定位（`repo_path` / `open_golden`）、 \
    transcript 编解码、临时目录（`temp_dir`）；
  - 各 crate 以 `dev-dependencies` 引入，本 crate 不依赖任何 hux crate（避免成环）。 \
    **方案专属夹具留在各自 `tests/`**；
  - 本 crate 超过约 200 行或承载业务逻辑即停手，退回各 crate 内 `#[cfg(test)]` 助手。
- `hux-bench` **不新建**： \
  基准以 `--release`示例提供（`crates/hux-scheme/tiger/examples/{decode_bench,key_bench}.rs`）， \
  避免新依赖（离线可构建）。**优化只允许「金样不变」的改动，且须有前后对比数据**； \
  用法、基线与结论见本文「性能」。
- CI：`rust` 作业（fmt / clippy / **分层测试**：
  - 内核+助手 → 方案 → 配置+平台 / core 平台痕迹与「core 无方案引用 /平台不引用方案内部」校验 \
    /数据溯源 / **金样 sha 表与内部头部校验**（`tools/checks/verify_golden_shas.py`）/ \
    **装-卸-CMake 清单一致自检**（`tools/checks/check_data_manifest.sh`）/两个一键脚本的`bash -n` + \
    `--dry-run` 冒烟）+ `addon` 作业（cmake configure 与构建链接、 \
    `hux_abi.h` ↔ `libhux.so` 符号一致、`DESTDIR` 安装布局 = 3 个插件文件 + \
    `data/MANIFEST`全部随包数据）+ 金样重生成比对（「层依赖」一步覆盖本文「结构与硬规则」规则 1 的四条边）；
  - `rust` 作业 16 步；
  - **待补**：`cargo-deny`（可选）、CI action 钉 commit sha（[`review-ledger.md`](review-ledger.md) \
    §0的 `[待办]`）；
  - Rust 工具链**有意跟随最新 stable**（不钉 `rust-toolchain.toml`）。

## 4. 依赖校验

> **统一做法**：源码文本类守卫一律先**剥离 Rust 注释**（`//`、`///`、`/* */` 含嵌套， \
> 字符串字面量保留）再匹配（`tools/checks/rust_source_grep.py --mode no-comments`）—— \
> 注释里写角色名不会让 CI 变红，真代码的字面量照旧命中；依赖边由 `cargo tree` 判定。

以下均已入 CI：

- `crates/hux-core` 不得出现环境变量读取 / `SystemTime` / `/usr/share` / `eprintln!` / \
  `println!` 等平台痕迹（`.github/workflows/ci.yml` 的 Core platform-clean；注释除外）；
- `crates/hux-core` 不得出现 `hux_scheme` / `hux-scheme` 引用（**注释除外**——说明性提及合法， \
  实测 6 处），也不得引用已迁出的方案模块（`decode` / `lexicon` / `lexical` / `ngram` / \
  `interaction` / 反查）；
- `cargo tree`：`hux-core` 无 `hux-scheme/*` 依赖边，且 `hux-scheme/*` 只依赖 `hux-core`；
- `platform/fcitx5/src` 的方案引用走**两级白名单**（模块 + 导入名）：
  - ① 方案模块只能是 `hux_scheme_tiger::scheme`（`hux_scheme_tiger::<其它模块>` 一律失败）；
  - ② 从 `scheme` 大括号导入的名字只允许 `ASSETS` / `TigerScheme` / `SCHEME_ID`；
  - ③ 保留内部模块黑名单（`interaction` / `decode` / `lexicon` / …）——即「平台经契约驱动」；
- `cargo tree`：`hux-cfg` / `hux-ffi` 不依赖方案（`hux-scheme/*`）与平台层（`hux-platform*`）—— \
  本文「结构与硬规则」规则 1 的四条边全部有守卫；
- `crates/hux-core` 不得出现**带引号的**角色名 / 方案选项键字面量（`"tab_learning"`、 \
  `"tiger_sentence_<…>"` 等）——角色词汇归 `hux-cfg`、键归方案； \
  rime 标准名 `full_shape` / `ascii_punct` 由 core 宿主链自持，不在此列；
- 后续可选 `cargo-deny`。

## 5. 骨架（已落地）

- 方案骨架：
  - `crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/`—— \
    见[`../crates/hux-scheme/README.md`](../crates/hux-scheme/README.md)；
  - 平台骨架：`platform/{windows,macos,ios}/`——见 [`../platform/README.md`](../platform/README.md)。 \
    每个骨架目录写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
  - **仅目录与说明，不进 workspace**，避免空壳死代码。

## 6. 模块映射（参照 → Rust）

| 参照                                                         | Rust                                                                                   | 差分手段                      |
| ------------------------------------------------------------ | -------------------------------------------------------------------------------------- | ----------------------------- |
| `lua/tiger_sentence_cache.lua`                               | `hux-core`: `cache.rs`                                                                 | fixture 金样（状态/淘汰序）   |
| `lua/tiger_sentence_ngram.lua`                               | `tiger/ngram.rs`                                                                       | `logp`/`obs`/`status` 逐位    |
| `lua/tiger_sentence.lua`（词库/解码/证据）                   | `tiger/lexicon.rs` + `tiger/decode.rs`                                                 | 数据索引 + 解码/证据/学习快照 |
| `lua/tiger_sentence_learning.lua`                            | `hux-core`: `learning.rs`（机制）<br>+ `tiger/interaction/learning_glue.rs`（策略）    | 检查重放 + learning 金样      |
| `lua/tiger_sentence_lexical.lua`                             | `tiger/lexical.rs`（TCSLEX01）                                                         | 词先验金样                    |
| `lua/tiger_sentence.lua`（processor/translator/filter/选项） | `hux-core`: `key.rs` + `session.rs`；<br>`tiger`: `interaction.rs`（+ `interaction/`） | 键序列金样                    |
| librime `key_event`/`key_table`                              | `hux-core`: `key.rs` + `key_table.rs`（由源码生成）                                    | 键金样（真 librime 探针）     |
| librime `reverse_lookup_translator`                          | `tiger/sound_to_char_shape.rs`（TCSRV01）                                              | 音反查金样                    |
| librime 宿主链                                               | `hux-core`: `host.rs` + `punct.rs`（提交点回调见 `CommitObserver`）                    | 键序列金样                    |

## 7. 数据与目录

- 目录解析在平台层（`platform/fcitx5/src/paths.rs`；内核不读环境变量）： \
  只读目录 `HUX_DATA_DIRS`（覆盖）> `$XDG_DATA_HOME/fcitx5/hux`（缺省 \
  `~/.local/share/fcitx5/hux`）> `$XDG_DATA_DIRS/*/fcitx5/hux`（缺省 `/usr/local/share`、 \
  `/usr/share`，末级 `/usr/share/fcitx5/hux`）；开发可用 `HUX_DATA_DIRS`（冒号分隔）与 \
  `HUX_MODEL` 覆盖；安装去向见 [`resources.md`](resources.md)「落点与查找顺序」。
- 运行数据：码表四件套 + 追加码表、模型、`symbols.yaml`、词先验、音反查索引、 \
  选项与学习库（文件名 / 格式 / 来源见 [`../data/README.md`](../data/README.md)、 \
  [`resources.md`](resources.md)）。

## 8. fcitx5 集成要点

- **注册与构建**：addon 元数据 + 输入法条目 conf，C++ 薄壳链接 Rust 静态库； \
  构建 / 安装与落点见 [`usage.md`](usage.md)「安装」。
- **会话**：每输入上下文一个（`InputContextProperty`；暂存隔离，选项为引擎级）， \
  失焦 / 切换见 [`platform/README.md`](../platform/README.md)； \
  组合重建由 `interaction::CompositionBuilder` 按参照 `Compose` 语义（`input[..caret]`、 \
  公共前缀增量保留、提交后旧段不复用）。
- **宿主语义**（core `host.rs`，`processor` 返回 Forward 后执行）：

  | 组件 | 行为要点 |
  |---|---|
  | `key_binder` | `Tab`→Down、`Shift+Tab`→Up（`when: has_menu`） |
  | `selector` | 菜单导航与翻页（`page_size`、上/下翻页键与翻页循环可由配置覆盖；<br>默认 `-`/`[` → Page_Up、`=`/`]` → Page_Down，**两侧同前置**：<br>菜单可见（且非 `ascii_mode`）即判翻页——用户决定 B，见下）、Home/End；<br>候选排列由配置写入 `_vertical`。<br>参照 `Selector::PreviousPage` 在首页也 `Highlight(0)`<br>（`menu/page_down_cycle` 只作用于 `NextPage`），本实现按参照；<br>参照另写 `paging` 标签，本仓随其唯一读取方一并删除 |
  | `navigator` | 字节光标移动；<br>Ctrl/Shift+Left/Right 跳到段首/段尾（未做音节 spans 细分）；<br>Home/End 到组合起点/末尾 |
  | `express_editor` | space 确认/提交、BackSpace 撤销编辑、<br>Delete 删光标处、Return 提交原文、Escape 取消；<br>可打印字符先提交组合再交宿主 |
  | `punctuator` | 单键可打印 ASCII 查 `symbols.yaml`；<br>组合中提交「组合文本 + 标点」；`{pair}` 交替 |

    > **翻页键不被标点分支遮蔽（本仓有意偏离上游 `abad411`）**： \
    > 判据 `hux_core::host::paging_action(...)` 两侧共用；**代价**：菜单可见时这几个键打不出标点。 \
    > 依据与最小复现见 [`upstream-deviations.md`](upstream-deviations.md) ①。

- **英文模式不实现**（设计取舍）：英文输入交由 fcitx5 切换输入法； \
  大写字母经 `char_handler` 直通（先提交组合）。
- **提交与按键顺序**：处置位 `HUX_KEY_FORWARD_AFTER_COMMIT` \
  （`crates/hux-ffi/include/hux_abi.h`）在提交后重发按键；语义与例外见 \
  [`platform/README.md`](../platform/README.md)。
- **UI 同步**：preedit 参照 librime `Composition::GetPreedit`——高亮候选的 `preedit`（按词分码如 \
  `sh ks`，反查段按音节）优先，其后原始输入原样接续（移动光标时保持分码， \
  如 `` ab cd `` + 尾部 `ja` → `` ab cdja ``）；无高亮候选回退「缓冲 + 原始输入」，光标为字节偏移。
- **反查**：`sound_to_char_shape.rs`（音）/ `char_to_sound_shape.rs`（字）对齐 librime 词典反查； \
  契约见 [`platform/README.md`](../platform/README.md)。
- **学习**：提交点通知器（参照 `Context::Commit`）覆盖核心路径与宿主链提交点； \
  `LiveLearning::submitted` 排空落库、`store_ready` 后生效，库 1 万条 / 16 MiB、60 秒节流。 \
  提交点范围见 [`platform/README.md`](../platform/README.md)，存储见 [`config.md`](config.md)。
- **选项键与配置**：方案经 `Scheme::option_declarations` 自报「角色 → 键」（角色词汇在 \
  `hux-cfg::roles`），平台装配处解析成角色表、**缺角色即报错**（状态串可见）； \
  配置项与选项存储（含只读回退 `user.yaml` 的 `var/option/*` 与保存失败属性 \
  `tiger_sentence_options_error`）见 [`config.md`](config.md)， \
  图形 schema 与平台接线见 [`platform/README.md`](../platform/README.md)。

## 9. 测试

1. **Rust 差分**：模块对金样逐位断言；
2. **键序列金样**：真 librime 探针生成「键序列 → 提交 / 候选 / 预编辑」，Rust 重放比对；
3. **CI**：fmt / clippy / 差分 + 固定参照提交重生成 fixture 金样比对。

清单、格式、重生成与 sha 校验见 [`goldens/README.md`](../goldens/README.md)， \
分层、CI 作业与工具链纪律见本文「测试与性能纪律」。

## 10. 性能

纪律见本文「测试与性能纪律」（**金样不变** + 前后对比数据）。 \
基准是两个 `--release` 示例（另有 `ngram_bench.rs`），不引入 `criterion`，保持离线可构建：

```sh
# decode 冷路径：重放 goldens/decode.tsv.gz 的 847 条输入（与差分测试同一批语料）
cargo run --release --example decode_bench
cargo run --release --example decode_bench -- --model goldens/ngram_fixture.bin \
    --lexical data/tiger_sentence.lexical.bin

# 整键路径：经方案契约驱动会话，测「process_key + rebuild」单键耗时
cargo run --release --example key_bench
cargo run --release --example key_bench -- --model goldens/ngram_fixture.bin
```

参数：`decode_bench` 支持 `--model <bin>` / `--lexical <bin>` / `--repeat N`， \
`key_bench` 支持 `--codes N` / `--repeat N` / `--model <bin>`。 \
输出：`decode_bench` 一行 JSON（`corpus`/`repeat`/`ops`/`mean_us`/`p50_us`/`p95_us`/`max_us` \
/`checksum`）再加按输入长度分桶的 5 行文本；`key_bench` 一行 \
JSON（`codes`/`repeat`/`keys`/`model`/`mean_us`/…，无 `checksum`）。 \
`checksum` 用于确认测量期间计算真的发生了且结果稳定。

**基线**（2026-09-21，开发机 Arch + release，`--repeat 20` / `key_bench --repeat 10`）：

| 场景 | p50 | p95 | max | 说明 |
| --- | --- | --- | --- | --- |
| decode 冷路径（无模型） | **1.00 µs** | 2.29 µs | 4779 µs | 16,940 次 |
| decode 冷路径（fixture 模型） | 1.06 µs | 2.56 µs | 2605 µs | 16,940 次 |
| decode 冷路径（+ 真实词先验位图） | 1.08 µs | 2.58 µs | 2671 µs | 16,940 次 |
| 整键路径（无模型） | **2.41 µs** | 181 µs | 2670 µs | 4,990 次 |
| 整键路径（fixture 模型） | 2.71 µs | 204 µs | 2871 µs | 4,990 次 |

按输入长度分桶（decode 冷路径，无模型）：

| 输入长度  | 样本   | p50         | p95      | max     |
| --------- | ------ | ----------- | -------- | ------- |
| 1–2 字符  | 14,080 | 0.96 µs     | 1.69 µs | 786 µs  |
| 3–5 字符  | 2,820  | 1.63 µs     | 3.38 µs | 10.3 µs |
| > 20 字符 | 40     | **2278 µs** | 2355 µs | 4779 µs |

**结论**：

1. 打字路径已是微秒级：1–5 字符（占语料 99.8%）decode ≤3.4 µs（p95）， \
   整键（处理器 + 宿主链 + 重建）p50 约 2.4 µs，相对键盘输入间隔（数十毫秒）可忽略， \
   **不构成优化理由**。
2. 尾部代价全部来自 >20 字符的长整句（p50 ≈ 2.3 ms）：beam 解码的固有工作量， \
   仍远低于交互预算（~10 ms），该形态本就少见。
3. 因此**不做**参照实现的「增量 / 锁解码缓存」：收益集中在长输入路径， \
   风险是该缓存需与解码 arena 的路径下标生命周期绑定（`decode.rs` 的 `Evaluated::path`）， \
   属「改动语义边界」的优化，不符合「金样不变 + 按需」的前提。
4. **复核触发条件**：① Android 中低端机实测长整句出现可感卡顿； \
   ② 输入长度上限（`MAX_RAW_LENGTH`）放宽；③ 模型从 mobile \
   换成更大模型——届时再做该项缓存并用本节基准给前后数据。

**维护约定**：改动 decode / 交互路径后跑一遍上面四条命令并与基线比对； \
**checksum 变化即为行为变化**，必须查清（差分金样也应报警）。 \
词库装载（构造期一次性）另有实测：全量装载 140–156 ms、仅主表约 17 ms， \
见 [`../data/README.md`](../data/README.md)「代价」。
