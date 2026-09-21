<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 重构：核心引擎化 / 平台无关 / 码表无关 / 测试正式化

目标：把现状（虎句 + fcitx5 桌面）整理为可承载**多方案、多平台**的引擎结构。 \
**本轮范围**：双端（linux / android）+ 虎码（字 / 词 / 句）；其他方案与平台仅留 README 骨架。

## 1. 结构正义（硬规则）

1. **依赖单向**：`platform/*` 依赖 `hux-ffi` / `hux-cfg` / `hux-core`，并在**装配处**依赖具体方案
   （`platform/* → hux-scheme/*`，只允许经契约与装配面常量，不得引用方案内部模块）；
   `hux-cfg → hux-core`；`hux-scheme/* → hux-core`；内核不依赖任何方案、不依赖任何平台。
2. **core 零平台**：不得出现 `std::env`、XDG / 绝对数据路径解析、`SystemTime::now`、`eprintln!`；
   路径 / 时钟 / 日志由平台构造并注入（目录列表 / `now: f64` / notes 汇总）。
   **允许**：读取平台传入的**显式路径**（如 [`PunctTable::load`] 按给定路径读 `symbols.yaml`）——
   core 不解析环境、不拼接平台目录，只做「给定路径 → 解析」的纯步骤；
   CI 的 platform-clean 检查据此只拦 env / XDG / 时钟 / 直接打印。
3. **职责归位**：只读数据（码表 / 模型 / 索引）属方案；可写数据（选项 / 学习库）属 `hux-cfg` 与平台存储实现；
   UI 快照、C++ 壳、打包属 `platform`；C ABI 是平台边界（`hux-ffi`）。
4. 模块名即职责；单元测试随模块、集成 / 差分测试独立目录（二者分离）。

## 2. 目标结构

```
crates/                       # 平台无关的 Rust 库
  hux-core/                   # 引擎内核：key/session/composition/处理器管线/宿主链/学习机制
                              #   + 方案契约（hux_core::scheme）
  hux-cfg/                    # hux 自身可配置项：设置项定义与默认值、选项存储与合并顺序
                              #   （options.yaml > 设置 > 内建）、状态菜单开关白名单、持久化接口
  hux-ffi/                    # C ABI：C 布局类型 + 导出函数（桌面 / Android 共用）
  hux-scheme/
    tiger/                    # 虎码（字/词/句）——本轮唯一全量实现
    yuhao/  wubi/             # init：README 骨架（形码族，复用 tiger 框架）
    shuangpin/  quanpin/      # init：README 骨架（拼音族，接口预留）
  hux-test-support/           # 测试助手（P5 已落地：金样路径 / transcript 编解码 / 临时目录）
platform/                     # 平台适配
  fcitx5/                     # 共享 fcitx5 适配：Rust 组装（Engine/UI 快照/存储实现/Paths）
                              #   + C++ 壳 + CMake（linux 与 android 共用）
  linux/                      # 桌面：构建 / 安装说明（入口脚本在仓库根；打包待做）
  android/                    # Android：构建接线（对接 fcitx5-android fork 的 plugin/hux）
  windows/  macos/  ios/      # init：README 骨架
```

平台层分工：`platform/fcitx5` 是**共用适配**（两端都是 fcitx5，环境变量与路径解析同一套）；
`platform/linux`、`platform/android` 只管各自的**构建与分发**。

> **现状（P4c 后）**：`crates/hux-cfg`、`crates/hux-ffi`、`crates/hux-scheme/tiger`、`platform/fcitx5`、
> `platform/linux` 均已落地；`hux-core` 只余通用内核（cache/key/key_table/learning/punct/session/host）
> **+ 方案契约 `hux_core::scheme`**；平台装配根构造 tiger 后以 `dyn Scheme` 驱动。

## 3. 迁移映射（P1–P3 已执行，留档）

| 现位置 | 去向 |
| --- | --- |
| `hux-core` 通用部分（key / session / punct / 处理器骨架 / host / 学习机制 / cache） | `crates/hux-core` |
| `hux-core` 方案部分（lexicon / decode / lexical / ngram / sound_to_char_shape / char_to_sound_shape / 虎码规则） | `crates/hux-scheme/tiger`（**P4b 已完成**） |
| `hux-addon/src/settings.rs`、`options.rs` | `crates/hux-cfg` |
| `hux-addon` 的 C ABI 段 | `crates/hux-ffi` |
| `hux-addon` 的 Engine / Session / UI 快照 / 存储实现（learning_store） | `platform/fcitx5`（Rust crate） |
| `crates/hux-addon/shell`（C++ 壳）与 `CMakeLists.txt` | `platform/fcitx5`（C++）+ `platform/linux`（构建） |
| 根目录 `install.sh` / `uninstall.sh` | **保留在仓库根**（P3 结论：脚本即构建入口，不移入 `platform/linux`） |

## 4. 批次

| 批 | 内容 | 验收 |
| --- | --- | --- |
| P0 | 本文档 | 评审通过 |
| P1 | 机械解耦：`hux-addon` 拆模块、`interaction.rs` 拆目录 | 行为 / API 不变；用例 + 金样全绿 |
| P2 | 平台承接环境耦合（数据目录解析、系统时钟、状态日志），清 core 的 env / XDG 硬编码 | 同一批验收；桌面与 Android 路径均由平台构造 |
| P3 | 拆 crate 与 `platform/`：`hux-cfg`、`hux-ffi`、`platform/fcitx5`、`platform/linux` + 骨架 README | workspace 编译通过；金样全绿 |
| P4 | `hux_core::scheme` 最小契约；`hux-scheme/tiger` 物理拆分 + 虎码 profile | 契约落地；不实现新方案 |
| P5 | 测试正式化：`hux-test-support`（✅ 收编两份 `tests/common/`）+ 单元 / 集成分离 + CI 分层（✅） | CI 全绿 |
| P6 | 性能（按需）：基准（✅ `--release` 示例）+ 增量解码缓存（按数据决定：**暂不做**） | 金样不变 + 有对比数据 ✅ |

> 进度：P0 ✅ → P1 ✅（`hux-addon` 拆模块、`interaction` 拆目录）→ P2 ✅（平台注入，core 去环境耦合）→
> P3 ✅（`hux-cfg` / `hux-ffi` / `platform/fcitx5` / `platform/linux` + 骨架 README）；
> P4 全部落地：**P4a ✅ 去回边**（`host::CommitObserver`）、**P4b ✅ 迁模块**（`hux-scheme/tiger`）、
> **P4c ✅ 契约注入**（`hux_core::scheme`，平台 `dyn Scheme` 驱动）、**P4 收尾 ✅**（选项 id 单一来源）。
>
> P5 全部落地：**`hux-test-support` ✅**（收编两份 `tests/common/`，全仓无 `mod.rs`）、
> **CI 分层 ✅**（内核+助手 / 方案 / 配置+平台 三步）。
>
> P6 已按「先测后优化」收口：基准与基线见 [`perf.md`](perf.md)；数据表明打字路径已是微秒级
> （1–5 字符 p95 ≤ 3.4 µs、整键 p50 ≈ 2.4 µs），尾部仅来自 >20 字符长整句（p50 ≈ 2.3 ms，仍 << 交互预算），
> 故**不做**参照的增量解码缓存（其收益集中于长输入，且牵涉解码 arena 路径下标生命周期），
> 复核触发条件记在 `perf.md`。
>
> **B2 基线（历史数值，见 §8「复核整改第 3b 批」与第 4 批的门槛行）**：
> `cargo test --workspace` **288 用例全绿**
> （core 72 / tiger 130 / cfg 16 / platform 65 / support 4 / ffi 1；
> 较 B1 的 281 增加 7 个：core 的 `fusion_keys_match_reference_vectors` /
> `fusion_score_is_pairwise_difference` / `fusion_event_encodes_direction_and_offsets`，
> tiger 的 `fusion_ordering_matches_reference_cases` /
> `fusion_ordering_preserves_direct_order_and_reverse_preference` /
> `processor_tab_confirm_stages_against_live_baseline`，
> platform 的 `host_commit_direct_choice_records_no_learning`），
> `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`reuse lint`（152/152）均干净；
> CI `rust` 作业 11 步本机逐步通过；C++ 侧构建链接 + `DESTDIR` 安装 3 文件 + 导出 **14** 个 `hux_*`。
> **当前基线（复核整改第 4 批实测）**：`cargo test --workspace --locked` **336 用例全绿**
> （core 84 / tiger 137 / cfg 20 / platform 76 / support 4 / ffi 1 及独立 `tests/`）；
> `rust` 作业 **16 步**（新增金样校验器、清单自检、入口冒烟三步）；
> `DESTDIR` 安装 **3 个插件文件 + 7 个随包数据文件**（`data/MANIFEST`）。

> **P4 落地记录**（`hux-scheme/tiger` 物理拆分；分批见下）：
> 1. 先定 `hux_core::scheme` 最小契约（内核必须回调的动作：`id` / 选项声明 / 按键与翻译重建 / 学习 / 反查），
>    不把虎码特有语义（缓冲态、锁、早提交启发式）泛化进契约；
> 2. 迁入 `crates/hux-scheme/tiger`：`decode` / `lexicon` / `lexical` / `ngram` / `sound_to_char_shape` /
>    `char_to_sound_shape`，以及交互中的虎码策略（`early_commit` 等，边界按契约定）；
> 3. 平台装配（`platform/fcitx5/src/engine.rs`）改为经契约注入 tiger；
> 4. 全程金样 + 用例守护（`cargo test`、`reuse lint`、C++ 链接）。
> 方向约束：`hux-scheme/* → hux-core`，内核不 import 任何方案（可加 CI 校验）。
>
> 分批（每批独立可验证）：
> - **P4a ✅ 去回边**：core 不再持有方案状态——`host::process_key` 的 `&SentenceState` + `LearningCommit`
>   两个方案参数，改为 core 定义的 [`hux_core::host::CommitObserver`] 回调，方案侧以
>   `interaction::HostCommitObserver` 实现（`session` 仅剩的 `interaction` 引用是文档链接，已改中性表述）。
> - **P4b ✅ 迁模块**：`crates/hux-scheme/tiger`（包 `hux-scheme-tiger`）已建：6 个计算模块 + `interaction/`
>   整体迁入，core 只留 `cache` / `key` / `key_table` / `learning` / `punct` / `session` / `host`；
>   差分包随模块走（`key` / `learning` 留 core）；`hux-cfg` 与平台改为依赖 core + tiger；
>   选项 id 归 `hux-cfg::options`（与方案的 `OPTION_*` 由平台一致性测试守护，P4c 并入契约）。
> - **P4c ✅ 契约注入**：`hux_core::scheme` 落地（`Asset` / `KeyOutcome` / `SessionId` / `SchemeConfig`
>   + `Scheme` trait）；`TigerScheme` 承载共享资源（解码器 / 标点表）与全部会话状态，
>   平台 `Engine` 只持有 `Box<dyn Scheme>` 与 `SessionId`，按键 / 候选 / 重建 / 学习 / 反查均经契约。
> - **P4 自审 ✅**（收尾一轮）：core 去掉最后的方案语义——`Context` 的缓冲态改内核布尔标记
>   （`set_buffered` / `is_buffered`），不再读 `tiger_sentence_buffered_text` 属性；
>   契约删除零调用的 `assets()`（资产清单归装配面常量 `ASSETS`，因模型须在构造前解析）；
>   `SchemeConfig` 删除未使用字段；`hux-cfg` 删除已迁入方案的 `learning_mode` 死代码。
> - **P4 收尾 ✅**：选项 id 纳入契约（`Scheme::option_ids` → `OptionIds`；两者**均已删**，见下行），
>   `hux-cfg` 的 4 个硬编码常量与平台的一致性测试一并移除，实现单一来源。
>   （该强类型结构已在 §8① 被**声明式契约**取代：`OptionIds`（已删）→ `Scheme::option_declarations`
>   + 角色表；历史形态仅此处留档，勿在代码中引用。）

## 5. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案无论如何要依赖 core 的类型（`KeyEvent` / `Context` / `Candidate`…），
  单开 interface crate 只多一跳、无净收益；将来若接口变大或需对外提供「方案作者 SDK」，
  再拆 crate（纯移动 + `pub use` 兜底）。
- **最小契约**：只定义内核必须回调的动作（`id` / 选项声明 / 按键与翻译重建 / 学习策略 / 反查展示；
  落地清单见下文「落地形态」）；
  **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约——先留在 `tiger` profile，
  等第二个同族方案落地后再抽象。
- 形码族（虎码 / 宇浩 / 五笔）优先；拼音族（双拼 / 全拼）只留接口。
- **选项键单一来源 + 角色归配置层（✅ §8① 已收口）**：方案经 `Scheme::option_declarations`
  自报「角色 → 键」声明（`&'static [OptionDecl]`）；**角色词汇与默认值归 `hux-cfg`**
  （`hux_cfg::roles` 的常量；宿主标准项 `full_shape` / `ascii_punct` 由配置层自持，不由方案声明）。
  平台在装配处把声明解析为 `OptionKeys` 角色表（**缺角色即报错**，不静默接线），
  `Settings::{option_defaults, store_defaults, option_default}` 与 `options::option_defaults`、
  `OptionsStore::load` 均按该表工作；平台的状态菜单白名单亦据此构造（角色序 = C ABI `HUX_OPTION_*` 序）。
  键的**持久化兼容**由方案侧测试 `option_declarations_are_stable_persisted_keys` 钉住，
  「每个角色都必须被方案声明」由平台测试 `every_configured_role_is_declared_by_the_scheme` 钉住，
  YAML 读写格式由 `hux-cfg` 的 store 测试（含历史键字面量）守护。
  **角色一致性守护（复核整改 §8① 后续）**：`hux-cfg` 与方案各自持有一份同值字面量，
  过去只靠人肉同步——`Config::parse` 对未知角色 `unwrap_or(0/false)` 静默回退，单侧改名可让
  `min_retained_raw_length` / `high_freq_limit` 静默失效而全绿。现在：
  ①方案自报 `hux_scheme_tiger::scheme::SCHEME_CONFIG_ROLES` 并由 `TigerScheme::load` 报出
  「未识别 / 缺少角色」诊断（进 `hux_engine_status`）；
  ②平台测试 `scheme_config_roles_match_the_scheme`（清单逐项比对 + 真实装配路径无诊断 + 改名必报诊断）、
  `runtime_role_tables_cover_the_declared_roles`（`SCHEME_OPTION_ROLES` ⊆ `RUNTIME_OPTION_ROLES`、
  存储 / 会话缺省覆盖运行时角色）、`option_role_order_matches_the_abi_header`
  （解析 `hux_abi.h` 的 `HUX_OPTION_*` 枚举序 ↔ `RUNTIME_OPTION_ROLES`）钉住全部四张清单；
  ③`hux_abi.h` 的 `HUX_OPTION_COUNT` + C++ `static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`
  把「加角色未补文案」从越界读（UB）变成编译失败。
- **口径命名（复核整改，范围 C）**：配置 / ABI / 平台层的**标识符**描述引擎概念
  （`ROLE_MIN_RETAINED_INPUT_LENGTH`、`ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS`、
  `ROLE_REVERSE_LOOKUP_CHARACTER_KEYS`、`ROLE_LEARNING_ON_TAB` 及对应的 `Settings` 字段 /
  `hux_options` 成员 / C++ 配置成员）；**线上字符串一律不动**——`ROLE_*` 的值仍是与上游 schema /
  rime 选项同名的键（`"min_retained_raw_length"` / `"sound_to_char_shape_keys"` /
  `"char_to_sound_shape_keys"` / `"tab_learning"`），`shell/hux.cpp` 的 `.path{}` / schema 默认值路径、
  `tiger_sentence_*` 前缀、学习库目录名、`options.yaml` 与 `Library` / `Icon` 也保持原样。
  方案侧的**模块 / 函数名**（`sound_to_char_shape` / `char_to_sound_shape` 及其内部 helper）
  是参照移植的溯源名，不在此列。
- **方案配置袋（✅ §8①）**：`SchemeConfig` 是「角色 → `Value`（开关 / 计数 / 文本 / 文本列表）」的
  **通用键值袋**——平台按角色装配（角色全集 `hux_cfg::roles::SCHEME_CONFIG_ROLES`，装配完整性由平台测试
  `scheme_config_covers_every_declared_role` 守护），方案按角色解释；内核不再出现
  `min_retained_raw_length` / 反查键 / Tab 学习等虎码口径字段，换方案不必改 core。
- **内核不 import 任何 `hux-scheme/*`**（校验方式见 §7）。
- **配置诊断通道（§8① / 第 2 批 F7）**：`Scheme::apply_config(&SchemeConfig) -> Result<(), Vec<ConfigError>>`
  ——`SchemeConfig::require_{bool,count,text,texts}` 把「角色缺失」与「类型不符」区分开
  （`ConfigError` 带角色名与期望类型），方案按缺省值回退的同时把诊断回给平台；
  平台并入状态串（`config:` 前缀，与装配期「未识别的角色 / 缺少角色」同风格）。
  `texts` 相应改为 `Option<&[String]>`（不再把「类型不符」退化成空切片）。
- **落地形态（P4c；§8① 后更新）**：`hux_core::scheme::Scheme` 只含「必须回调方案」的动作——
  `id` / `option_declarations` / `learning_mode` / `apply_config` / `host_options` /
  `set_store_ready` / `apply_learning_index` / `new_session` / `free_session` /
  `reset_session` / `process_key` / `select_candidate` / `rebuild` / `take_learning_events` /
  `buffered_text` / `auxiliary_lookup_active` / `auxiliary_rows`；
  学习 mode 由方案据配置袋**自算**（平台只取不透明串 `Scheme::learning_mode`），
  虎码特有语义（缓冲态、锁、早提交启发式、证据、mode 串格式）全部留在 `TigerScheme` 内部。

## 6. 测试与性能

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle。
  现状（复核整改第 4 批复测）：源内单测 **24 个文件**（内核 7 / 方案 8 / 配置 4 / 助手 1 / 平台 3 / ffi 1），
  另有 2 个同目录的独立 `tests.rs` 模块文件（`tiger/src/interaction/tests.rs`、`platform/fcitx5/src/tests.rs`），
  集成与差分 7 个在 `tests/`（内核 2 / 方案 5）——两者不混放。
  （漂移来源：§8① 新增的 `hux-core/src/scheme.rs`、`hux-cfg/src/roles.rs` 单测；数法为
  `grep -rl '#\[cfg(test)\]' crates platform`。）
- `hux-test-support`（P5，**✅ 已落地**，`crates/hux-test-support`）：只放与业务无关的共性工具——
  金样 / 夹具路径定位（`repo_path` / `open_golden`）、transcript 编解码、临时目录（`temp_dir`）；
  各 crate 以 `dev-dependencies` 引入，本 crate 不依赖任何 hux crate（避免测试期成环）。
  **方案专属夹具留在各自 `tests/`**（如 `decode_differential.rs` 的 `make_decoder`）；
  若本 crate 超过约 200 行或开始承载业务逻辑，立即停手、退回各 crate 内 `#[cfg(test)]` 助手。
- `hux-bench` **不新建**：基准以 `--release` 示例提供（`crates/hux-scheme/tiger/examples/{decode_bench,key_bench}.rs`），
  避免新依赖（保持离线可构建）；基线与结论见 [`perf.md`](perf.md)。
- 优化只允许「金样不变」的改动，且须有前后对比数据。
- CI 现状：`rust` 作业（fmt / clippy / **分层测试**：内核+助手 → 方案 → 配置+平台 /
  core 平台痕迹与「core 无方案引用 / 平台不引用方案内部」校验 / 数据溯源 /
  **金样 sha 表和内部头部校验**（`tools/checks/verify_golden_shas.py`）/
  **装-卸-CMake 清单一致自检**（`tools/checks/check_data_manifest.sh`）/ 两个一键脚本的
  `bash -n` + `--dry-run` 冒烟）
  + `addon` 作业（cmake configure 与构建链接、`hux_abi.h` ↔ `libhux.so` 符号一致、
  `DESTDIR` 安装布局 = 3 个插件文件 + `data/MANIFEST` 全部随包数据）+ 金样重生成比对
  （「层依赖」一步覆盖 §1 规则 1 的四条边）；复核整改第 4 批后 `rust` 作业为 16 步；
  **待补**：`cargo-deny`（可选）、依赖/工具链钉版本（§8「工具 / CI」的 `[待办]`）。

## 7. 依赖校验

> **统一做法（复核整改第 4 批 C6/C7）**：源码文本类守卫一律先**剥离 Rust 注释**（`//`、`///`、`/* */`
> 含嵌套，字符串字面量保留）再匹配——工具是 `tools/checks/rust_source_grep.py`
> （`--mode no-comments`，正负例见 `_tmp/批次4-文档工具CI.md`）。故「注释里写为什么不能出现
> 某个角色名」不再让 CI 变红，而真代码里的字面量照旧命中；依赖边判定仍由 `cargo tree` 负责。

- ✅ 已入 CI（`.github/workflows/ci.yml` 的 Core platform-clean）：`crates/hux-core` 源码不得出现
  环境变量读取 / `SystemTime` / `/usr/share` / `eprintln!` / `println!` 等平台痕迹（注释除外）；
- ✅ 已入 CI：`crates/hux-core` 不得出现 `hux_scheme` / `hux-scheme` 引用（**注释除外**——
  `lib.rs` / `scheme.rs` / `host.rs` / `session.rs` 里「不依赖 hux-scheme」的说明性提及是合法的，实测 6 处），
  也不得引用已迁出的方案模块（`decode` / `lexicon` / `lexical` / `ngram` / `interaction` / 反查）；
- ✅ 已入 CI：`cargo tree` 校验 `hux-core` 无 `hux-scheme/*` 依赖边，且 `hux-scheme/*` 只依赖 `hux-core`；
- ✅ 已入 CI：`platform/fcitx5/src` 的方案引用走**白名单**（复核整改第 4 批 C7，此前只是「内部模块黑名单」）：
  ① 出现的方案模块只能是 `hux_scheme_tiger::scheme`（装配根构造方案，`hux_scheme_tiger::<其它模块>` 一律失败）；
  ② 从 `scheme` 大括号导入的名字只允许 `ASSETS` / `TigerScheme` / `SCHEME_ID`；
  ③ 保留原有内部模块黑名单（`interaction` / `decode` / `lexicon` / …）——即「平台经契约驱动」；
- ✅ 已入 CI：`cargo tree` 校验 `hux-cfg` / `hux-ffi` 不依赖方案（`hux-scheme/*`）与平台层（`hux-platform*`）——§1 规则 1 的四条边全部有守卫；
- ✅ 已入 CI（§8① 新增）：`crates/hux-core` 不得出现**带引号的**角色名 / 方案选项键字面量
  （`"tab_learning"`、`"tiger_sentence_<…>"` 等）——角色词汇归 `hux-cfg`、键归方案；
  rime 标准名 `full_shape` / `ascii_punct` 由 core 宿主链自持，不在此列（**注释与文档叙述确实不受影响**：
  守卫剥离注释后匹配，复核整改第 4 批 C6 修正了此前 `grep` 注释误伤的措辞/实现不一致）；
- 后续可选 `cargo-deny`。

## 8. 复核遗留（P0–P6 全仓复核后登记，按需排期）

> 2026-09-21 全仓复核（5 路并行审计 + 人工核实）已修项见提交 `chore(review)` 三批与
> `fix(review)`。
>
> **条目状态前缀（复核整改第 4 批 D1–D6 引入，逐条与代码/测试对齐）**：
> `[✅ 已修]` ＝ 已落地且有守护（用例 / CI 守卫 / 金样）；`[待办]` ＝ 仍未做（含成本估计）；
> `[已登记·不修+理由]` ＝ 评估后**有意不改**，理由随条目给出。
> **不再保留「文档说未做、代码已做」（或反之）的条目**：每批追平/整改收尾时勾对一次。
> `[误报·已核实]` ＝ 审计结论被实测否掉（反证随条目给出）——仅用于本节末的「复核整改总账」。

**方案（tiger）追平上游 ✅ 已完成**（分批推进：**B1 ✅ → B2 ✅ → B3 ✅ → B4 ✅ → B5 ✅**；
起点 pin `8b615235`，区间共 **31 笔**（24 笔非合并提交）。收尾时**两个 pin**：

- **主干 pin `abad411750f79cfca750985fa266689b5d9b865f`**（`origin/main` 尖端）；15 份由 Lua 核心生成的
  夹具 / decode / learning / lexical 金样与键序列探针金样 `key_sequence.tsv.gz` 取自该 pin；
- **反查分支尖端 `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`**（`feat/reverse-lookup`；`4ff37c4` 数字选择器
  提交反查候选、`92a0b54` 撇号音节分隔），它是主干 pin 的**后代**，故音反查金样 `sound_to_char_shape.tsv.gz`
  单独取自它、**不再做「分支 + 主干本地合并」**（生成器已简化为单 `PIN` + 显式失败护栏）。

详见 `_tmp/批次1..5-追平记录.md`；金样来源与 sha 表见 `goldens/README.md`。

- **丙：模型与格式侦察 ✅ 已完成**
  - **格式未变**：上游 README 仍写「TCSKNM02 分页格式」；`lua/tiger_sentence_ngram.lua`
    在 pin..HEAD 间**仅 2 行差异，且均为 Windows 开发路径字面量**（非格式改动）。
    ⇒ 本实现「只接受 TCSKNM02 mobile」的约束**无需任何新格式支持**。
  - **模型为同名新训练**：默认模型自 2026-09-20 起为 `full-kn-m5-v2`
    （469,886,928 B / 448.12 MiB，sha256 `c0063898fdff27c1fb00c1c72fa28d…`），
    另有文档化的 fused 变体（224,475,584 B / 214.08 MiB）；**是否换模型由用户决定**，与代码追平解耦。
    上游 README 第 74 行仍称模型文件为 `sentence-ngram-mobile.bin`（TCSKNM02，**约 448 MiB**）——
    文件名、格式、量级三者与上述新默认一致，故**换模型 = 替换同名文件，零代码改动**。
- **B1 ✅ 已完成（pin `8b615235` → `201eb79`）**——只移植 `5ce1ca2`（+其后的测试提交 `ff3895d`
  与 PR #17 合并 `201eb79`），**跳过融合特性**：
  - 阈值 `early_commit_minimum_share` 0.995→**0.99**、`early_commit_strong_share` 0.99999→**0.999**
    （后者只用于 `base_share`；空码强阈值另立 `empty_code_strong_share` = 0.99999）；
  - 新增 `supplement_early_commit_scale` / `supplement_early_commit_cap` /
    `personalized_early_commit_cap` / `empty_code_strong_share`，以及
    `supplement_early_commit_contribution()` 与 `early_confidence()`
    （优先 `early_commit_confidence_score`，回退 `confidence_score`）；
  - `evaluate_state` 新增 `early_commit_confidence_score` = 置信度 + `min(0.80, 补充码表贡献 + 学习奖励)`；
    `build_prefix_evidence` 拆 `base_weight`/`early_weight`（`share` 用早提交权重、`base_share` 用基础权重，
    `boundary_share` 仍用基础权重）；截断池不再早退（保留 `BaseShare` 供强证据策略），
    `try_early_commit` 只在「学习生效且截断」时拒绝，且截断时要求 `base_share ≥ 强阈值`；
  - `learning.reward()` 改**三元返回**（新增 `learning_early_bonus` → item 的
    `learning_early_commit_bonus`），并新增 `early_commit_maturity`/`early_commit_contribution`
    与 `reinforce()`（未按 Tab、提交项即本菜单首选时走稳定观测增量，不再记成纠错）；
  - **金样**：pin 推进到 `201eb79` 并重生成全部参照金样；`ngram_fixture`/`lexicon`×4/`lexical`
    逐字节不变，`decode*.tsv.gz` 仅新增 `early_commit_confidence_score` 字段，
    `decode_evidence*`（截断池证据）与 `learning`（三元奖励 + 成熟度 + `reinforce`）按上述行为变更；
    `key_sequence`/`sound_to_char_shape` 仅头部 pin/sha 变化（行为内容逐字节一致）。
    反查分支（`2c53111`/`f3b3049`/`ce5b840`/`898579f`）**不在 `201eb79` 的祖先链上**
    （上游至今未并入 main），本实现仍按 `8b615235..898579f` 的单文件 +48/−18 逐处核对为「已覆盖」。
- **B2 ✅ 已完成（pin `201eb79` → `bd83900`）**——区间 `201eb79..bd83900` 的**运行时代码只有 6 笔**
  （其余 7 笔为测试/CI/合并提交，逐笔判定依据见 `_tmp/批次2-融合偏好-分析.md` §1）：
  - `24e633e`：`learning` 侧新增 `fusion_mode`/`fusion_pair_code`/`fusion_score`/`fusion_event`
    （成对偏好：`D`/`C` 两个 choice 竞争，单次确认权重 1 ⇒ `min(16, 9+2ln w)`）；
  - `a3fc009`：**直接序保持 + 跨来源融合**——`State` 记 `source_mask`/`direct_rank`
    （`direct_edge = previous == nil and position == 0 and whole_input_edge`）、
    同文本多路径 `source_union`/`min` 聚合并回写 arena、`evaluate_state` 对 Direct 剥离学习分、
    `emit` 末尾按 `direct_rank` 重排 Direct 列后与 Composed 列做**两指针归并**（前缀前瞻；
    两侧都不 `> 0` 或差值 ≤ `1e-12` 时回退原下标序）；
  - `c69c1a8`：**与 composed 自学习分离**——差异/稳定确认只在 composed-only↔composed-only 之间产生，
    模式串 `sentence-v1` → `sentence-v2`（旧记录仍留在库中但索引不再命中）；
  - `6ece735`：Tab 锁确认分支携带 `_fusion_ahead`（竞争者随 `seen` 浅拷贝）；
  - `681c9c8`：把上述 helper 挂到 `learning.*` 表——**Lua 每函数 200 local 上限的技术搬迁，无语义差**
    （Rust 侧未复刻该写法）；
  - `59fc87a`：`fusion_pair_code` 加 `"~f"` 前缀，把融合码移出 raw 前缀索引命名空间
    （虎码 raw 只含 `a-z`，而 `~` > `z` ⇒ 融合码恒排在 raw 码之后、`code_window` 遇首个 `~` 即停）。
  - **顺带修的既有缺陷**（独立提交）：Tab 锁确认路径**先清 `tab_pending` 再补 stage** 导致学习基线取成
    `submitted_first` 并可能误走 `reinforce`；现照参照在清标志前 stage，并有 `processor` 端到端用例
    `processor_tab_confirm_stages_against_live_baseline`（金样覆盖不到该路径：探针 `store_ready == false`）。
  - **金样**：pin 推进到 `bd83900` 并重生成全部参照金样。实证与预期一致：
    `ngram_fixture`/`lexicon`×4/`lexical`/`decode`/`decode_model`/`decode_rank_first`/
    `decode_evidence`/`decode_evidence_model` **逐字节不变**；
    `decode_learning`/`decode_learning_model` 按 Direct 剥离与排序变化（`learning=<0|1>` 与
    `score`/`learning_score`/`early_commit_confidence_score` 字段），并在生成器里多播一条
    「Composed 胜」的成对偏好（`zzzz` 的 `哥哥` 越过 `𨰻`），使归并分支在解码金样里可见；
    `key_sequence`/`sound_to_char_shape` 仅头部 pin/sha 变化（行为内容逐字节一致）；
    `learning` 新增融合记录（`fusionmode`/`paircode`/`fusion`/`fusionnone`/`fusionevent`，
    含 `full` 与 `runtime` 两种索引形态；生成器与差分解析同批扩展）。
- **B3 ✅ 已完成（pin `bd83900` → `b2bbd23`）**——区间内**运行时代码只有 1 笔**：
  `b228c2d` perf(lua) reduce evidence allocation and repeated fusion work（`b30187c` 只改 README 的模型说明，
  `b2bbd23` 为 PR #19 合并）。逐行判定为**纯分配/重复计算优化、行为中性**，**Rust 侧无代码改动**
  （仅 `crates/hux-core/src/learning.rs` 一条注释里的 pin 更新）；细节见 `_tmp/批次3-追平记录.md` §2：
  - `dedup_limit`：本调用私有的 `result` 数组直接发布（不再复制第二份 `limited`），
    `reserved`/`kept` 表只在真有保留项时构造——空表 `table.sort` 与空 `ipairs` 本就是空操作；
  - `build_prefix_evidence`：权重改为「边遍历边累加」（候选迭代序与浮点加法序不变），
    `base_total/early_total <= 0` 早退从遍历前移到遍历后（两项各 ≥ `exp(0)=1`，该分支恒不成立——已实测 847 用例 0 次触发）；
  - `evaluate_evidence_state`：部分尾证据改用只算置信度的轻量求值器（不再算 `path_isolation_penalty`/
    `max_rank`/`score`）；证据池只读 `text`/`path`/`confidence_score`/`early_commit_confidence_score`，
    两个置信度字段与 `evaluate_state` 逐位相同（上游 `tools/test_allocation.lua` 直接断言这一点）；
  - `State.text_char_count`：字符数与字节长度并行累加（`utf8.len` 可用时才填，否则回落 `utf_length`，LuaJIT 路径不变），
    消除 `build_prefix_evidence`/tracker 处的重复 `utf_length`；
  - `apply_fusion_ordering`：无 Direct 的菜单直接早退（原路径该分支本就原样返回），
    并对同一次合并内的 `(direct, composed)` 对做记忆化（键 `(d-1)*#composed+c` 无碰撞，零值同样入缓存，
    表随调用私有、不跨合并复用）。
  - **金样零变化**：pin 推进到 `b2bbd23` 并重生成全部由 Lua 核心生成的 17 份金样，**非注释内容逐字节一致**
    （14 份 fixture/decode/learning/lexical 的 `gunzip | cmp` 与 `ngram_fixture.bin` 全部相同）；
    只有 `key_sequence`/`sound_to_char_shape` 的头部 pin 与 lua sha **各 2 行**变化（行为内容逐字节一致）。
  - **覆盖负向验证**（实测）：把 `expand_range` 的字符数 `+1` ⇒ `decode_evidence` 失败；
    污染融合对缓存值 ⇒ `decode_learning` 失败（144 条 `mit=1` 证据用例覆盖部分尾求值路径）。
  - **工具**：`gen_sound_to_char_shape_golden.sh` 的本地合并新增冲突处理——主干与反查分支都往
    `tools/run_regressions.py` 的用例清单追加过条目而必然冲突，该文件不参与生成故取分支侧版本；
    **其余任何**冲突一律显式失败，避免静默产出与主干行为不符的金样。
  - **门槛**：`cargo test --workspace --locked` **288 用例 0 失败**（与 B2 基线同数；本批无 Rust 改动，故未新增用例）；
    fmt/clippy/reuse 152/152/CI 分层守卫全绿；pin/sha 程序化自验 0 处不一致。

- **B4 ✅ 已完成（pin `b2bbd23` → `d7b01e5`）**——区间内**运行时代码只有 2 笔**：
  `7b220ce`（经 `068936b` 分支合并落地）与 `d30867a`（`d7b01e5` 合并）；两个合并提交相对各自父提交在 `lua/` 上无独立差异
  （`068936b -p1` 即 `7b220ce` 的净效果，`d7b01e5 -p1` 即 `d30867a` 的净效果）。细节见 `_tmp/批次4-追平记录.md`。
  - `7b220ce` **持久化人工纠错等级**：`lua/tiger_sentence_learning.lua` 由「时间衰减 + 连续权重」改为
    **离散等级**——每次人工纠错 `weight = min(10, weight + 1)`，同上下文分 `7 + 2L`（L1=9…L10=27）、
    跨上下文分 `4 + 2L`（L1=6…L10=24），等级之外手工竞争项仍 `×0.25` 降权；
    **学习不再随时间衰减**（时间戳只作持久化元数据），`M.build` 的 `now` 参数与 `refresh_scores` 因此都退化为元数据，
    `update_index` 删除「时钟回退/未来事件 ⇒ 全量重放」判据，`early_commit_maturity` 改为 `clamp((score-9)/4)`（L1/L2/L3 → 0/0.5/1）；
    **删除 `M.reinforce`**——未按 Tab 的首选重复确认不再计入等级（上游测试 `ordinary learned first choice never reinforces`）。
    **持久化格式未变**：仍是「时间 / mode / code / text / context」五元组 frame，`#f == 5` 校验原样，
    等级由事件条数即时推导，故旧库直接兼容（无需新字段/版本/迁移）。
  - `d30867a` **竞争切分前瞻保护**：新增 `competing_boundary_end(raw, committed_raw, proposed, target_text_elements)`——
    保留量必须按**已输出的文本元素数**对齐比较（`nv` 提交「有」时要等到 `nvt` 的「郁」也攒够前瞻）；
    两处调用点（`try_commit_mature_prefix` 的 retain 判据、`try_empty_code_commit` 的 `min_retained` 判据）
    由裸 raw 长度改为该边界；`retain_trackers_without_counting` 新增 `reset_maturity`：
    低置信度（`neutral_low_confidence`）的比较型缺口把 `evidence_count`/`strong_count` 清零（只保身份）。
  - **金样**：pin 推进到 `d7b01e5` 并重生成全部 Lua 核心金样。`ngram_fixture`/`lexicon`×4/`lexical`/`decode`/
    `decode_model`/`decode_rank_first`/`decode_evidence`/`decode_evidence_model`/`ngram_fixture.bin` **逐字节不变**；
    `decode_learning`/`decode_learning_model` 按「无时间衰减」重算学习分（单笔纠错由 `9+2ln(w)` 的衰减值恢复为整 9，
    13 边长句的 `learning_score` 由 ≈14.3 变为 84.0 = 各段等级分之和）；
    `learning` 改为等级语义并新增 `levels_full`/`levels_runtime`/`levels_aged` 三个索引
    （守护 +2/级、10 级封顶、以及「时间推后 10 年分值不变」），`reinforce*` 记录随 `M.reinforce` 一并移除；
    `key_sequence`/`sound_to_char_shape` 仅头部 pin/sha 变化（行为内容逐字节一致）。
  - **覆盖缺口已补**：`key_sequence`/`sound_to_char_shape` 的合成夹具里竞争边界恒等于 tracker 边界，
    故 `d30867a` 的调用点金样覆盖不到 —— 补了 3 个单测：`competing_boundary_end` 用上游
    `tools/test_tiger_sentence_incremental.lua` 的三条真实向量（`jreynvtah`/`jreynvtahx`），
    `mature_prefix_waits_for_the_competing_boundary` 直接驱动 `try_commit_mature_prefix`
    （负向控制：改回裸 raw 长度即失败），`retain_trackers_resets_maturity_only_for_low_confidence_gaps`。
  - **门槛**：`cargo test --workspace --locked` **291 用例 0 失败**（B3 基线 288；+4 新用例、−1 随 `M.reinforce` 删除的用例；
    分项：core 72 / tiger 133 / cfg 16 / platform 65 / support 4 / ffi 1；
    `learning_stage_reinforces_stable_first_choice` 改写为 `learning_stage_does_not_reinforce_stable_first_choice`）；
    fmt/clippy/reuse 152/152/CI 分层守卫全绿；pin/sha 程序化自验 0 处不一致。
- **B5 ✅ 已完成（收尾批；主干 `d7b01e5` → `abad411`，反查支线 `898579f` → `92a0b54`）**——本批两个来源
  （详见 `_tmp/批次5-追平记录.md`）：
  - **主干 `abad411`**（`fix(rime): preserve punctuation learning and default to full-m5`，区间仅此 1 笔）：
    处理器把「缓冲态标点先冲组合」的判据由 `state.buffered_text ~= ""` 改为 `context:has_menu()`，
    并在确认前补 `learning_selection` + `learning_stage`——标点段一旦追加进组合，`learning_selection`
    就无法再解码该输入（如 `zhhbi,`）或取回句子的选中项，故**标点路径的人工纠错得以保留**。
    另：默认模型改为 `full-kn-m5-v2`（469,886,928 B，sha256 `c0063898…`，文件名与 TCSKNM02 格式不变；
    **本仓不捆绑模型**，只同步文档表述）；`lua/tiger_sentence_ngram.lua` 的改动仅 Windows 开发路径字面量。
  - **反查两笔**（`4ff37c4` 数字选择器提交反查候选、`92a0b54` 撇号音节分隔）：反查段内数字直选
    （`index = digit-1`，越界惰性消费）、`;` 惰性不并入拼音；识别模式改为 `^`[a-z']*$`
    （撇号可出现在任意位置，`speller/delimiter` 增加 `'`），处理器保留撇号。
  - **金样**：15 份 Lua 核心金样与 `ngram_fixture.bin` 在 `abad411` 下**逐字节不变**；
    `key_sequence` 仅新增用例（`punct_menu_equal`/`punct_menu_minus`，记录下面的遮蔽行为——其中
    `punct_menu_equal` 现为**本仓有意偏离**项，见下方「⚖️ 有意偏离上游」）；
    `sound_to_char_shape` 取 `92a0b54`：`digit-zhong` 由「数字并入拼音」变为**直选提交**，
    `nav-page-equal`/`nav-page-minus`/`nav-page-zho` 由「`=`/`-` 翻页」变为「先上屏组合再落标点」，
    并新增撇号/数字/分号用例（31 例 / 164 步）。
  - **⚠️ 参照行为变化（已由用户判定为上游缺陷 ⇒ 本仓有意偏离）**：`abad411` 的标点分支使**所有**可打印 ASCII 标点
    （含 `-`/`=`/`[`/`]`）在菜单可见时先确认组合、再交标点表 ⇒ 方案侧 `key_binder` 的 `-/=`、`[/]` 翻页绑定
    在这条路径上被遮蔽（`Page_Up`/`Page_Down` 与 `Tab` 循环不受影响）。这是上游参照的实测行为
    （探针金样 `punct_menu_*` 与 `nav-page-*`），本仓当时如实移植；**现按用户判定修掉该缺陷**——
    这是金样层面唯一的偏离登记，做法与豁免项见下方「⚖️ 有意偏离上游」。
  - **门槛**：`cargo test --workspace --locked` **291 用例 0 失败**（与 B4 同数：仅改写 2 个平台用例、
    扩 1 个断言表、金样用例内新增覆盖，无净增删）；fmt/clippy/reuse/CI 分层守卫全绿；
    另复验 C++ addon 的 configure→构建→`DESTDIR` 安装布局与 **14 个** `hux_*` 导出符号、
    `gen_pinyin_index.py --check`。

**⚖️ 有意偏离上游 ✅ 已实施**（金样记录上游行为、**字节不动**；偏离项一律按「本仓逐步期望值 + 期望差异集合 ==
实测差异集合」登记在 `crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的 `DEVIATIONS`，
其余金样用例无条件逐位比对。偏离共三类：**上游缺陷修复**（下表 ①）、**addon 扩展**（②）、
**pin 差异**（③）。复核整改 3a 把登记从「跳过名单」升级为**可证伪的期望值表**，
3b 再把 `DEVIATED_CASES` 更名/扩展为 `DEVIATIONS`（见 `_tmp/批次3a-tiger整改.md`）。

### ① 参照缺陷：菜单可见时 ASCII 翻页键被标点分支遮蔽（用户判定为上游缺陷）

- **缺陷**（上游 `abad411` `fix(rime): preserve punctuation learning …` 起）：方案处理器在 `context.has_menu()`
  时对**所有**可打印 ASCII 标点先「暂存学习 + 确认组合」再交标点表；而翻页绑定在宿主 `key_binder`
  （缺省 `-`（`when: paging`）/`=`（`when: has_menu`），schema 可绑 `[`/`]`）。于是菜单可见时这些键
  **永远轮不到**翻页绑定；`Page_Up`/`Page_Down`/`Tab` 不经该分支，不受影响。
- **最小复现**：`j a` 后按 `equal` ⇒ 期望下翻一页，上游提交「一=」（金样 `punct_menu_equal` 如实记录）；
  音反查 `` ` z = = `` / `` ` z = = - `` / `` ` z h o = - `` 同理（`nav-page-equal`/`nav-page-minus`/`nav-page-zho`）。
- **本仓修法**（判据复用，避免两处条件漂移）：core 抽出**唯一**判据
  `hux_core::host::paging_action(context, options, key_event) -> Option<PagingDir>`
  ——`Up` = 命中 `page_up_keys` 且末段带 `paging` 标签（参照 `when: paging`），`Down` = 命中 `page_down_keys`
  （参照 `when: has_menu`），前置条件 `!ascii_mode && has_menu`；`key_binder` 与方案标点分支**共用**它：
  标点分支入口先问一次，判为翻页则**不消费**（不 stage 学习、不确认组合），键落回宿主链执行翻页。
  `ProcessorEnv` 新增 `host_options: &HostOptions`，平台 `TigerScheme` 把与宿主链同一份绑定传进处理器。
- **不受影响的路径**（逐条单测）：无菜单、非标点键、编辑/导航键、`Page_Up`/`Page_Down`、`Tab`/`Shift+Tab`、
  缓冲态标点，以及**未翻页的 `-`**（`when: paging` 不成立 ⇒ 仍确认组合 + 落标点，与上游一致）。
- **金样策略（字节零变化）**：金样记录的是**上游行为**，故 `goldens/*.tsv.gz` 一个字节都不改；
  偏离用**单一期望值表**表达：`crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的
  `DEVIATIONS`（一个常量同时服务两份金样 `key_sequence` / `sound_to_char_shape`），每项 = 金样名 +
  **本仓逐步期望值**，并断言「期望 ≠ 金样」的步集合**恰好等于**「实测 ≠ 金样」的步集合
  （既证明确有差异，也防止实现回退成上游后静默通过）；反向断言「登记的偏离已消失」即红。
  文档侧的单一来源是 `goldens/README.md` 的偏离说明（`docs/refactor.md` 本节给理由与沿革）。
  登记名必须真实存在；不得静默跳过其它用例：

- **偏离表（三类合并；`steps` 列 = 金样步数，偏离步为「本仓期望 ≠ 金样」的步）**：

  | 类 | 金样 | 偏离用例 | 步数 | 偏离步 | 上游 vs 本仓 |
  |---|---|---|---|---|---|
  | ① | `key_sequence.tsv.gz` | `punct_menu_equal` | 3 | 2（`equal`） | 提交「一=」 vs **翻页**（不提交） |
  | ① | `key_sequence.tsv.gz` | `nav_page_home_minus` | 4 | 3（`minus`） | `Page_Up` 停在首页后提交「乙-」 vs **上翻页**（不提交） |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-equal` | 4 | 2,3（`=`） | 上屏「中=」再落「=」 vs **连翻两页** |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-minus` | 5 | 2,3,4（`=`/`=`/`-`） | 同上＋落「-」 vs **翻两页后上翻一页** |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-zho` | 6 | 4,5（`=`/`-`） | 上屏「中哦=」再落「-」 vs **下翻一页后上翻一页** |
  | ② | `key_sequence.tsv.gz` | `digit_menu_select` | 3 | 2（`1`） | 数字并入编码 vs **直选当前页候选并上屏** |
  | ③ | `key_sequence.tsv.gz` | `apostrophe_digit_page` | 5 | 4（`Page_Down`） | 末段 abc 单段 ⇒ 消费 `Page_Down` vs 末段 raw ⇒ **不消费** |
  | ③ | `key_sequence.tsv.gz` | `apostrophe_semicolon_page` | 5 | 4（`Page_Down`） | 同上（`'` + `;` 变体） |

  其余用例（含 `punct_menu_minus`、`nav_page`/`nav_page_big`、`nav-page-keys`、`punct_half_shape_minus`、
  `punct_buffered_period`、`upper_*`，以及第 2 批新增的 `editor_ctrl_return`/`editor_ctrl_shift_return`/
  `punct_mid_caret_*`/`nav_page_up_home_reset`）**仍逐位一致**。
  `nav_page_home_minus`（`a b Page_Up minus`）是同源缺陷的另一条最小复现：`Page_Up` 停在首页写
  `paging` 标签（参照无条件写，第 2 批 F2）后，`-` 本仓走上翻页；上游因方案处理器在确认组合时
  `_auto_commit` 立刻提交并清空组合、标签随之消失，`-` 落标点提交「乙-」。
- **待上游修复后回归（①）**：上游若采纳下列任一改法，随下一批追平时**删除对应登记项**
  （含 `registry_is_falsifiable` 自校验用例覆盖）、恢复无条件逐位比对，
  并复验平台用例 `menu_paging_keys_are_not_shadowed_by_the_punctuation_branch` 的 5 条断言。

### ② addon 扩展：数字直选（`tiger_sentence_digit_select`，出厂缺省 `true`）

- 上游方案核心没有该选项（数字作为编码字符入串）；本仓按出厂缺省开启 ⇒ 菜单可见时数字直选当前页候选
  （`processor` 的 `select_page_candidate`，走与 `space` 相同的确认/学习链）。金样如实记录上游行为，
  重放按出厂缺省驱动（复核整改 3a / B1），差异登记为 `AddonExtension`（`digit_menu_select`）。
- 平台侧由状态菜单开关（`hux_engine_option_value + HUX_OPTION_DIGIT_SELECT`）；关闭后不再直选。

### ③ pin 差异：分段常量 `SEGMENTATION_DELIMITER`（`92a0b54` 的 `" '"` vs 主干 `abad411` 的 `" "`）

- **上游依据**：`92a0b54`（反查分支尖端，`4ff37c4`/`92a0b54` 两笔）把撇号写进
  `speller/delimiter`（`" "` → `" '"`）作为音节分隔符；本仓音反查语义（识别模式 `^`[a-z']*$`、
  撇号保留在输入、反查段内数字绝对索引、`;` 惰性）都追平该尖端，分段常量随之取 `" '"`。
- **影响面**：仅「`'` + 数字/`;`」序列。本仓把 `ab'1` 切成 abc 段 `ab'` + raw 段 `1`，上游主干是单段 `ab'1`。
  **段结构本身不在比对面**（`preedit` 按设计不比对）⇒ `apostrophe_digit_split`/`apostrophe_semicolon_split`
  两例逐位通过；可见差异是**末段类型**：raw 末段（无菜单）⇒ `Up`/`Down`/`Page_*` **不被消费**。
- **探针实测（决定性证据）**：同一探针（系统 librime 1.17.0）以 `PIN=92a0b54` 重跑
  `tools/generators/gen_key_sequence_golden.sh`，得到的行与本仓**逐位相同**（`Page_Down` 后 `consumed=0`）
  ⇒ 该差异是**上游自己后续提交**带来的，不是本仓发明；改回 `" "` 等于回退上游改动。
- **覆盖与守护**：金样新增 `apostrophe_digit_page`/`apostrophe_semicolon_page`
  （只增不改：旧内容是新文件的**严格前缀**；66 例/275 步 → **68 例/285 步**，sha 见 `goldens/README.md`），
  登记为 `BranchPinDelimiter`；`abc_segmentor_splits_after_a_delimiter_before_a_digit` 单测钉住
  `abc_segmentor` 的断段行为（`'`+数字/`;` 断开、`'`+首字母单段）。
  负向对照：把常量改回 `" "` ⇒ 两条登记项报「登记的偏离已消失」并失败。
- **待上游并入后的回归做法**：`feat/reverse-lookup` 并入 main 后，随下一批追平**重生成 `key_sequence` 金样**
  （两 pin 合一，`apostrophe_*_page` 记录的行为与本仓一致）⇒ 删除这两条登记项，恢复无条件逐位比对；
  届时 `tools/cases/key_sequence_cases.txt` 的偏离注释同步改写。
  （另注：尖端还有 `punct_segmentor` 把反查段里的 `;` 直接落成全角「；」——本仓的标点由宿主表处理、
  不实现该分段器，属**另一处**已知范围差异，与本项 delimiter 差异无关。）
- **撇号音节切分（A4 口径）与上游 librime 依赖**：`92a0b54` 的「按 `speller/delimiter` 切分音节」
  依赖上游 librime 的 delimiter 修复 [rime/librime#1233](https://github.com/rime/librime/pull/1233)；
  本机 librime 1.17.0 未含该修复，故已入库音反查金样里含撇号的段**无候选**（`apostrophe-*` 三例）。
  本仓只落地「识别模式放行 `^`[a-z']*$` + 撇号保留在输入中」，**不实现音节切分**——
  反查段由本段独占、音节按拼写键前缀建边，而拼写表不含 `'` ⇒ 行为与金样一致（注释见
  `sound_to_char_shape::matches_pattern`）。**回归做法**：上游 librime 修复并入后，
  重生成 `key_sequence`/`sound_to_char_shape` 两份探针金样并复验 `apostrophe-*` 用例；
  若届时上游同时并入 `feat/reverse-lookup`，③ 的两条登记项一并删除。
- **建议的上游修法**：
  1. 最小改动：`lua/tiger_sentence.lua` 的 `context:has_menu()` 标点分支入口先问一次 key_binder 的翻页判据
     （`page_up_keys`/`page_down_keys` 及其 `when` 条件），命中则直接 `return 2`（不 stage 学习、不确认组合）；
  2. 结构性改动：把 `key_binder` 提到方案处理器之前（librime 处理器链顺序调整），标点分支即无需感知翻页绑定；
  3. 兼容性约束：两种改法都须保住 `abad411` 的初衷——**未翻页的 `-` 仍落标点**、缓冲态标点的学习保留不得回退。

**平台（fcitx5）**（状态前缀：`[✅ 已修]` 已落地并有守护 / `[待办]` 未做 / `[已登记·不修+理由]` 评估后不改）
- `[✅ 已修]`：C++ 壳不再硬编码方案选项名——ABI 新增 `hux_engine_option_role_count` /
  `hux_engine_option_key(role)`（角色序 `HUX_OPTION_*`），状态菜单按角色取名、只保留 UI 文案；
  面板数字序号改取**运行时生效值**（`hux_engine_option_value` + `HUX_OPTION_DIGIT_SELECT`），
  状态菜单关掉数字直选后不再显示序号。导出 12 → 14，头文件与 `.so` 符号校验同步通过。
- `[✅ 已修]`：17 项默认值在 C++ schema 与 `hux-cfg::Settings::default` 各写一份的问题，
  由新增测试 `schema_defaults_match_settings_defaults`（解析 `shell/hux.cpp` 的
  `.path{}`/`.defaultValue`，含 keysym→rime 键名转换）逐项比对并断言核对数为 17，
  使漂移在 CI 即失败。
- `[✅ 已修]`（复核整改 F2）：状态菜单文案表 `kLabels[role]` 按 ABI 角色下标取，此前与
  `HUX_OPTION_*` 零绑定（加角色即越界读 UB、调序即菜单错位）。现 `hux_abi.h` 增 `HUX_OPTION_COUNT`、
  C++ 加 `static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`、Rust 用例
  `option_role_order_matches_the_abi_header` 解析头文件枚举序并与 `RUNTIME_OPTION_ROLES` 逐项比对
  （顺序 / 个数 / 名字 / 下标连续）。
- `[✅ 已修]`（复核整改 F3）：学习库读入的坏帧不再 panic（`learning::unframe` 改 `value.get(a..b)?`）——
  此前 LevelDB 任意值经 `from_utf8_lossy` 后若长度前缀落在 UTF-8 字符中间，`hux_engine_new`
  （`extern "C"`）即 abort；现坏帧跳过并计入既有 `error` 诊断（`hux_engine_status` 可见），库仍可用。
- `[已登记·不修+理由]`：**页大小**仍是只读配置（`PageSize` 只在配置页改、重启生效），而引擎其余运行时选项
  （数字直选等）走状态菜单。理由：核心 `host::page_size` 是构造期/重建期参数，运行时改页大小要么中断
  当前候选页、要么让面板与引擎页大小不一致；`docs/config.md` 亦未承诺可运行时改。
  （**面板数字序号**此前与之并列登记为只读，实为已修：见上方 `hux_engine_option_value` + `HUX_OPTION_DIGIT_SELECT`
  一条——复核整改第 4 批 D3 删去这条自相矛盾的登记。）
- `[✅ 已修]`：无会话时状态菜单切换直写存储落盘（`OptionsStore::set_value` + 单测）。
- `[✅ 已修]`：`CString` 含 NUL 时剔除并记日志（`ui::cstring_lossy` + 单测），不再整条丢空；
  事件泵上限具名为 `EVENT_PUMP_ROUNDS`。
- `[✅ 已修]`：保存失败属性 `*_options_error` 并入状态串（`hux_engine_status` 可见）+ 端到端单测。
- `[✅ 已修]`（第 2 批 F7）：方案 `apply_config` 的**逐角色诊断**（角色缺失 / 类型不符）经
  `Engine::apply_scheme_config` 并入状态串（`config:` 前缀）；此前运行期重新下发配置袋时
  任何漂移都静默回退。平台用例 `scheme_config_diagnostics_reach_the_status_string` 覆盖
  「真实装配无诊断 / 类型不符可见 / 漏装可见 / 恢复后清空」四段。
- `[✅ 已修]`（本轮顺带发现的真缺陷）：参照的选项抑制是**写入时**抑制（`live.syncing`），
  本移植曾用「按名一次性名单」——`sync` 排队的事件会**吞掉紧随其后的第一次真实改动**
  （状态菜单在会话建立后首次切换可能不落盘）。现改为写入后 `Context::discard_option_events`
  丢弃自身事件，`Options::observe` 回归参照语义；cfg 单测同步重写。
- `[✅ 已修]`（第 4 批 F5）：`cmake --install` 现在按 `data/MANIFEST` 一并安装随包数据
  （此前只有 `install.sh` 装数据 ⇒ 只走 CMake 安装得到「无词库」引擎）。`install.sh` 装后逐条核对，
  `uninstall.sh` 按同一清单删除（M15：此前枚举 7 个文件名 + 安装侧 glob ⇒ 漏删风险），
  自检 `tools/checks/check_data_manifest.sh` + CI 的 `DESTDIR` 步骤守护「装 / 卸 / CMake 一致」。
- `[✅ 已修]`（第 4 批 F6）：运行期学习库写入失败（`confirm` → `db.put` 错误）进状态串
  （`learning:` 前缀）——此前 `learning.error` 只在构造期读一次，运行期失败完全静默。
  平台用例 `learning_write_failure_reaches_the_status_string` 钉住接线。
- `[✅ 已修]`（第 4 批 F8）：`hux_engine_status` 的指针承诺改为**「在下一次状态刷新前有效」**
  （`hux_abi.h` + `abi.rs` 文档同步；实现本就是 `refresh_status` 替换 `CString`）。
  平台用例 `status_pointer_must_be_read_again_after_a_refresh` 钉住「刷新后重新调用即最新」。
- `[✅ 已修]`（第 4 批 F15）：配置页绑到**无名字 keysym**（媒体键等）时该绑定会被丢弃，
  此前无任何诊断；现 `Engine::apply_settings` 经 `unparsable_key_bindings` 把 `hotkeys: 忽略无法识别的绑定
  <角色>=<键名>` 写进状态串（同 `*_options_error` 风格），C++ 壳在应用设置后落 `FCITX_INFO` 日志；
  `platform/fcitx5/README.md` 的「已知限制」写明该行为与可用键名范围。
  平台用例 `unparsable_hotkey_binding_reaches_the_status_string`（正负例 + 恢复清空）。

**① 契约去虎码语义 ✅ 已完成**（跨 4 crate；实施记录见下）
- 病（原登记；下文的 `OptionIds` 与固定字段 `SchemeConfig` 均已**删除**，仅作历史对照）：
  `OptionIds` 的 4 个字段名与 `SchemeConfig` 的 `min_retained_raw_length`/反查键/Tab 学习
  仍是虎码口径，与 §5「不把方案特有语义泛化进契约」有张力（换方案须改 core）。
- 实施（数据化声明；强类型检查落在调用侧）：
  1. **core**：新增 `OptionDecl { role, key }`、`Value { Bool, Count, Text, Texts }` 与
     `SchemeConfig` 键值袋（`new` / `with` / `set` / `get` / `bool` / `count` / `text` / `texts` / `roles`；
     `Default` = **空袋**）；`Scheme::option_declarations() -> &'static [OptionDecl]`；
     删除（以下名字**均已成历史**，全仓代码零命中，仅本文件与本段留档）`OptionIds` / `Scheme::option_ids` /
     `Scheme::learning_mode(rules, duplicate, hfl)` / `Scheme::set_learning_mode` / `Scheme::learning_rules`（**无兼容垫片**）。
     注意区分：`tiger` 内部仍有字段 `lexicon.learning_rules`（参照移植的溯源名，**不是**契约方法）。
     学习 mode 改为方案自算 + 不透明 getter `Scheme::learning_mode(&self) -> &str`，
     `apply_learning_index` 随之去掉 `mode` 参数（平台不再持有 mode 串）。
  2. **hux-cfg**：新增 `roles` 模块——角色常量（`ROLE_*`）+ `SCHEME_OPTION_ROLES` /
     `HOST_OPTION_ROLES` / `RUNTIME_OPTION_ROLES`（= ABI 角色序）/ `SCHEME_CONFIG_ROLES` +
     `OptionKeys::resolve(&[OptionDecl]) -> Result<Self, DeclError>`（缺角色 / 重复声明 / 空项 → `Err`）。
     `Settings::{option_defaults, store_defaults, option_default}`、`options::option_defaults`、
     `OptionsStore::load` 改为按**已解析的角色表**工作；宿主标准项 `full_shape` / `ascii_punct`
     由本层自持（键 = 角色名，不由方案声明）。
  3. **tiger**：`option_declarations()` 自报 4 项（键 = `interaction::OPTION_*`，角色 = cfg 常量**同值字面量**
     ——方案不依赖 cfg，方向 `hux-scheme/* → hux-core` 与 CI 守卫不变）；新增内部 `Config::parse(&SchemeConfig)`
     按角色解析袋，虎码语义（早提交最短保留 / 反查键 / Tab 学习 / 页大小与翻页 / mode 串格式）全留在本 crate。
   4. **平台**：装配处 `resolve_option_roles(scheme.option_declarations())`——**全有或全无**：
      `OptionKeys::resolve` 发现任一问题（缺角色 / 重复 / 空声明）即 `Err`，平台随之不接线**任何**角色
      （诊断进状态串并列出问题清单；ABI `hux_engine_option_key` 返回 NULL、宿主跳过全部角色菜单项，
      不静默落到别的键上），而不是「只让出问题的那个角色不接线」；`hux_engine_option_role_count()` 与角色序同源。
     一致性测试升级：`every_configured_role_is_declared_by_the_scheme`（每个角色都必须被方案声明，
     含负例）、`scheme_config_covers_every_declared_role`（配置袋覆盖角色全集、顺序一致）、
     `option_role_keys_follow_scheme_declarations`（角色序 ↔ 键，17 项 schema 比对不变）。
- **与 §8 原方案的偏离（均按「目标不变、改动更小」）**：
  - `Value` 增加 `Texts(Vec<String>)`：翻页 / 反查键是**多项键列表**，用单一 `String` 需 join/split 往返；
    `Text` 保留但当前无角色使用（通用容器词汇，非虎码语义）。
  - 学习 mode 的下沉对象是**方案**而非 cfg：mode 串格式（`sentence-v2|rules=…|optimal=…|dup=…`）是虎码口径，
    放 cfg 等于把方案语义搬进配置层；且 `Settings::learning_mode` 在 P4 已删（本就不存在）。
    故 `Scheme::learning_mode` 由「固定入参计算」改为「不透明 getter」，输入经配置袋下发。
  - `apply_learning_mode`（已删 API）的 `mode` 参数、`Scheme::learning_rules`（已删契约方法）一并删除：
    平台不再需要 mode 串（`tiger` 内部的 `lexicon.learning_rules` 字段保留，见上）。
- **口径名残留（复核整改后状态）**：**标识符已中性化**——`hux-cfg::Settings` 的字段、
  `roles` 常量、`crates/hux-ffi` 的 `HuxOptions` 与 `hux_abi.h` 的 `hux_options` 成员、
  `shell/hux.cpp` 的配置成员名，均改为引擎概念名（见 §5「口径命名」）；
  **线上字符串保留**：`ROLE_*` 的值（= 上游 schema / rime 选项键 `min_retained_raw_length` 等）、
  `shell/hux.cpp` 的 `.path{"MinRetainedRawLength"}` / `"SoundToCharShapeKey"` / `"CharToSoundShapeKey"` /
  `"TabLearning"`、fcitx5 配置页文案与 `tiger_sentence_*` 前缀不变（改值即破坏与上游互通 / 老用户配置 / ABI 布局）。
  **`crates/hux-core` 零残留**（CI 守卫钉住），`hux-scheme/tiger` 的模块 / 函数名保持参照移植的溯源名。
- **门槛（本批实测）**：`cargo test --workspace --locked` **303 用例 0 失败**
  （基线 295：+2 core 袋用例、+1 cfg 角色解析用例、+1 cfg 设置表用例、+1 tiger 袋解析用例、
  +2 平台角色 / 配置袋用例；其中 1 个用例为改写更名）；`cargo fmt --all --check`、clippy `-D warnings`、
  `reuse lint` 与 CI 分层守卫（含新守卫，正负例均已验证）全绿；`goldens/**` **零改动**
  （`jj diff --stat` 无 `goldens/` 数据文件），差分金样逐位一致；C++ `addon` 作业本机复跑
  （configure → 构建 → 14 个 `hux_*` 导出 ↔ `hux_abi.h` → 探针语法 → `DESTDIR` 三文件）通过。

**复核整改第 2 批：hux-core 真缺陷 ✅ 已完成**（依据 `_tmp/复核-core.md` 的 F2–F12；
实施记录与逐条「参照依据 → 修法 → 守护 → 负向对照」见 `_tmp/批次2-core整改.md`）

参照源码：librime pin `33e78140` 的 `gear/selector.cc`、`gear/editor.cc`、`gear/punctuator.cc`、
`key_binder.cc`、`context.cc`、`composition.{h,cc}`、`segmentation.cc`、`engine.cc`（`ConcreteEngine::Compose`）。

- **F2（high）`Page_Up` 停在首页不写 `paging` 标签** → 按参照补齐：`Selector::PreviousPage`
  在 `selected_index < page_size` 时 `Highlight(0)`（**照常归零高亮**）并**无条件**写标签。
  此前默认配置下「`Page_Up` 后按 `-`」不判翻页，落标点分支把组合提前上屏。
- **F3（medium）`page_cycle` 首页上翻回卷** → 按参照删除该分支：`menu/page_down_cycle`
  只在 `NextPage` 被读；`docs/config.md`、`docs/rust-migration.md` 同步说明「只作用于下翻」。
- **F4（medium）`Ctrl+Return` 语义错** → 按 `Editor::CommitScriptText` 提交
  `Context::GetScriptText()`（`Composition::GetScriptText(keep_selection = true)`：确认段取候选文字，
  否则候选 `preedit` 去首个 `\t`，否则原始输入切片）并走 `Clear()`——**不发提交通知**
  （此前提交候选文字并写学习库）。§8「待复核：`Context::GetScriptText` 的准确定义」就此结案。
- **F5（medium）`Ctrl+Shift+Return`** → 按 `Editor::CommitComment`：注释为空时只吞键，
  不清组合、不提交空串。
- **F6（medium）`punctuator` 的 caret 语义** → 按 `Punctuator::ProcessKeyEvent` ＋
  `ConcreteEngine::Compose`（`active_input = input[..caret]`）：在 caret 处 `PushInput(ch)`，
  提交文本只取到该段末尾——光标居中时标点后的剩余输入随 `Clear()` 丢弃
  （此前提交整串，实测 `a b Left comma` → 参照「a，」 vs 本仓「ab，」）。
- **F7（medium）契约袋无错误通道** → `SchemeConfig::require_{bool,count,text,texts}`
  ＋ `ConfigError{Missing,TypeMismatch}`；`texts` 改 `Option<&[String]>`；
  `Scheme::apply_config -> Result<(), Vec<ConfigError>>`，方案（`Config::parse`）逐角色回诊断，
  平台 `Engine::apply_scheme_config` 并入状态串（`config:` 前缀，与装配期诊断同风格）。
  「角色名拼错 / 类型不符」不再静默回退。
- **F8/F9/F10/F11/F12（low）** → `build_valid` 同义包装删除；`LearningIndex::future` 注明为
  参照遗留元数据；`PunctTable::load` 收进 `#[cfg(test)]`（生产走 `load_first`）；
  `paging_action` 的 `kWhenPaging` 分支去掉多余的 `has_menu`/`ascii_mode` 前置（`kWhenHasMenu` 才要求）；
  `Context::highlight` 对未建立菜单的段不写不改不通知。顺带把 `context_valid` 的两次 `chars()` 合并为一次（F13.2）。
- **覆盖（金样）**：`tools/cases/key_sequence_cases.txt` 新增 6 例——
  `editor_ctrl_return`、`editor_ctrl_shift_return`、`punct_mid_caret_comma`、`punct_mid_caret_no_menu`、
  `nav_page_up_home_reset`（以上 5 例逐位一致）与 `nav_page_home_minus`（上游行为，登记
  `DEVIATED_CASES`，见上方偏离表）。`key_sequence.tsv.gz` 用主干 pin 的参照核心重生成：
  **57 例 / 242 步逐位不变**（含头部 4 行），仅新增 6 例 → **63 例 / 264 步**，
  sha256 `f4b032c3…`（`goldens/README.md` 与 CI 硬编码值同步）。
  宿主链绑定（金样走不到）另有 core 单测与平台端到端用例钉住。
- **负向对照（实测）**：把每处修复逐个改回缺陷形态后，对应守护全部失败——
  F2 标签 → core `page_up_at_first_page_arms_the_paging_binding` + 平台
  `menu_paging_keys_are_not_shadowed_by_the_punctuation_branch` 第 ⑥ 段；
  F2 归零高亮 → `key_sequence` 金样 `nav_page_up_home_reset`；
  F4/F5/F6 → 各自 core 单测 + 金样用例；F12 → core `highlight_skips_untranslated_segment_without_update`；
  F7 → 平台 `scheme_config_diagnostics_reach_the_status_string`。详见批次记录。
- **门槛**：`cargo test --workspace --locked` **322 用例 0 失败**（基线 311：core 74→84 = +10、
  平台 72→73 = +1；方案 121 / cfg 20 / ffi 1 / support 4 不变）；
  `cargo fmt --all --check`、clippy `-D warnings`、`reuse lint`（153/153）与 CI 分层守卫全绿；
  C++ `addon` 作业本机复跑（configure → 构建链接 → 14 个 `hux_*` 导出 ↔ `hux_abi.h` →
  两个探针 `g++ -fsyntax-only` → `DESTDIR` 三文件）通过。

**复核整改第 3b 批：tiger 剩余项与遗留收口 ✅ 已完成**（依据 `_tmp/复核-tiger.md` 的 B2/B4/B5/B6/A3/A5–A8/C7
与 6 条已知遗留；逐条「审计条目 → 判定 → 守护 → 负向对照」见 `_tmp/批次3b-tiger收口.md`；
门槛：`cargo test --workspace --locked` **333 用例 0 失败**，fmt/clippy/reuse 与 CI 分层守卫全绿）

- **B2 `SEGMENTATION_DELIMITER` 判定为「保留 + 登记为有意偏离（pin 差异）」**：见上方
  「⚖️ 有意偏离上游」③（上游依据、探针实测、影响面、覆盖与回归做法）。**改回 `" "` 的方案已实测否决**：
  它等于回退上游 `92a0b54` 的改动，并与音反查金样的 pin 冲突；且本仓行为与尖端探针逐位一致。
- **B6 融合事件两侧补覆盖（并修一处重复计数）**：产出侧（`learning_stage` 的 fusion 分支）与接受侧
  （`learning_submit` 的 `accepted_by_fusion`）原先在 tiger crate 内零用例（`fusion_ahead` 处处 `Vec::new()`）。
  新增两条**端到端**用例（`processor` → 宿主链 → `CompositionBuilder::rebuild` → `update_notifier`，
  真实菜单 + 数字直选）覆盖 Direct 胜 / Composed 胜两方向与接受；`raw_end` 保留语义、
  `#pending < 256` 上限、融合事件绕过子串过滤、提交文本不匹配丢弃各一条 learning_glue 级用例。
  **顺带发现并修掉一处真缺陷**：`select_candidate_at`（候选点击 / addon 数字直选）在
  `confirm_selection` 之前多暂存了一次，与参照「候选点击只经提交通知器一次」不符 ⇒ 同一次点击写
  **两条**相同成对偏好（权重记两次）。现删除该显式暂存（端到端用例的负向对照：保留重复时
  `submitted.len() == 2`）。
- **A3 上下文属性层只写不读 ⇒ 删除死路径**：`SentenceState::load` / `read_locks` /
  `parse_committed_property` 的唯一调用者是单测；`K_LOCKS`/`K_COMMITTED` 快照与 4 个 legacy 键的
  迁移/清理无任何生产读取方（`grep` 覆盖 `.rs/.cpp/.h`：FFI / 平台 / C++ 壳均无）。参照的属性往返源于
  Lua `env` 无状态，本仓会话状态由方案对象持有 ⇒ 按 A3 选项 ② 删除 `load`/`read_locks`/
  `parse_committed_property`/`save_locks` + `K_LOCKS`/`K_COMMITTED*`/legacy 键 + `legacy_cleared`；
  属性层只留**宿主与内核共享**的 `K_BUFFERED`（`select`/`early_commit`/`learning_glue` 与
  `Context::is_buffered` 都读）。将来若需跨进程/宿主恢复状态，须**显式**重新引入（含读取方）。
- **B4 覆盖下限断言**：两份探针金样的重放加下限（`key_sequence` ≥68 例/≥285 步、重放面 ≥63 例/≥265 步；
  `sound_to_char_shape` ≥31 例/≥164 步、重放面 ≥28 例/≥149 步）——金样被截断/少解析不再静默通过。
  负向对照：截断 `key_sequence.tsv.gz` ⇒ 「用例数不足 0 < 68」失败。
- **B5 ngram 真实模型差分在 CI 恒跳过（登记 + 闸门）**：`ngram_sample_transcript_is_bit_exact_when_present`
  在缺 `goldens/local/ngram_sample.tsv.gz`（不入库）或 448 MiB 真实模型时 `eprintln! + return`
  ⇒ CI 对真实模型路径**零守护**（decode 走 17 KB fixture）。本地复验按 `goldens/README.md` 的 sample
  生成命令产出抽样金样后 `cargo test -p hux-scheme-tiger --test ngram_differential`；
  本批新增 `HUX_REQUIRE_SAMPLE=1` 闸门（缺失即**失败**，供本地/专项 CI 强制覆盖；缺省仍跳过，
  保住无模型环境的全绿）。**建议（未做，成本登记）**：CI 加可选作业下载模型 + 缓存后跑该闸门——
  需 ~448 MiB 下载与缓存 action，与既有「不加缓存 action」的取舍冲突，留待用户决定。
- **A5–A8/C7（low）**：`MobileModel::load` 复用 `configure_cache` 的上限校验（非法上限返回错误，
  不再在 `Fifo::new(0)`/`Columns::new(0)` 的 `assert!` 处 panic）；`tri_ctx_count` 的 `u64→usize`
  改显式转换 + 「上下文数不得超过文件可容纳条目数」校验（32 位/android 目标不再静默截断），
  单字索引区尺寸用 `checked_mul`、索引字节数全程按 `u64` 计算；NaN 语义两处
  （`reward_for_weight` 的 `clamp`、`logp` 的 `max`）**只注明差异**（正常数据不可达、无金样支撑）；
  `tracker_better` 加 `text` 字典序兜底使三元平局确定化（与调用点排序结果一致，判据自身反对称）；
  音反查索引的读音串改为**按需构造**（真实索引 600,869 组里仅 412 组含单字词条）——
  实测首次加载 96–99 ms → **81–88 ms**（release，各 3 次取样，`data/tiger_sentence.pinyin.bin.gz`）。
- **死代码 / 重复清理（只删确无生产者消费者者）**：删除 10 项无使用者 `pub` API
  （`MobileModel` 的 7 个访问器、`SoundToCharShapeIndex::group_count`、`K_OPTIONS_ERROR`、
  `Decoder::{clear_learning, learning_index_mut}`）；`lexicon::candidate_paths` 收进 `#[cfg(test)]`；
  删除只写不读字段（`Lexicon::{codes_path,ranks_path,whitelist_path}`、`Supplement::path`、
  `LexicalModel::path`、`MobileModel::{path,source_index_bytes}`、`Evaluated::lexical_score`）、
  恒 0 的 `Chunk.cursor`、`sound_to_char_shape` 的死赋值与不可达分支说明；
  合并 **crate 内**重复常量（`CANDIDATE_LIMIT` 三份 → `decode` 一份、`EARLY_COMMIT_MINIMUM_SHARE`
  两份 → `decode` 一份）；`ngram_bench` 的私有 hex 解码改用 `hux_test_support::decode_hex`、
  `decode_bench` 删掉压警告的 `let _ = LEXICAL_FILE;`。
  **跨 crate 重复只报告不合并**（`lexicon::candidate_paths` ↔ `hux_core::scheme::asset_paths`；
  `interaction::state::{live_input,input_caret}` ↔ `hux_core::session::{live_input,live_caret}`；
  `ngram::BOS/EOS`（`&str`）↔ `decode::BOS/EOS`（`char`）——三者合并都牵动契约或热路径类型，另行排期）。
- **6 条已知遗留的结论**（逐条见 `_tmp/批次3b-tiger收口.md`）：
  ① 融合子串过滤无专用用例 → **已闭合**（B6 的
  `fusion_events_pass_the_filter_but_unrelated_diff_events_are_dropped`）；
  ② 无 `--learning 1 --early-commit 1` 组合金样 → **已闭合**（第 5 批 `5304bd357903`：新增
  `goldens/decode_learning_evidence.tsv.gz`（`--early-commit 1 --required 1 --learning 1`，12257 行）
  + `gen_decode_golden.lua` 的组合开关 + 差分位级比对 + CI 重生成比对；覆盖 `learning=1 && truncated=1`
  的截断池与 `share`/`base_share` 双权重交互——重生成命令、覆盖点与门槛见 `_tmp/批次5-收尾.md`、
  `goldens/README.md`）；
  ③ Tab 锁无真机探针 → **已闭合**（第 5 批 `5304bd357903`：新增 `goldens/key_sequence_tab.tsv.gz`
  （8 例 / 44 步）+ 夹具 `goldens/key_sequence_tab/`（`tiger_sentence.custom.yaml` 把
  `tab_learning: true` 这一**条件本身**入库）+ 独立生成器
  `tools/generators/gen_key_sequence_tab_golden.sh`；重放侧 `store_ready = true` 并断言
  `captures == 8`——注意**增量是「参照 Tab 基线分支真的走进去 + 重放侧真的接上 `learning_selection`」**，
  比对字段在 `tab_learning` 两侧逐位相同，不构成新的可见行为覆盖）；
  ④ `SEGMENTATION_DELIMITER` → **已闭合**（本批 B2，登记为 `BranchPinDelimiter` 偏离）；
  ⑤ `learning_potential` 未随 Direct 剥离 → **仍成立但为空操作**（Direct 边两项皆 0，仅当将来给 Direct 赋非零 potential 才可见）；
  ⑥ root `learning_score` 恒 0 的构造性依赖 → **仍成立**（两侧同构；风险仅在将来给 root 写非零学习分时）。

**复核整改第 4 批：文档 / 工具脚本 / 金样机制 / CI 守卫 / 平台杂项 ✅ 已完成**（依据
`_tmp/复核-文档工具CI.md` 的 D1–D14 / M1–M17 与 `_tmp/复核-cfg-平台.md` 的 F5/F6/F8/F15；
逐条「审计条目 → 修法 → 守护 → 负向对照」见 `_tmp/批次4-文档工具CI.md`，门槛：
`cargo test --workspace --locked` **336 用例 0 失败**，fmt / clippy `-D warnings` / `reuse lint` /
CI 分层与新增守卫全绿，本地复跑 `rust`（16 步）与 `addon` 作业）

- **金样机制（M1/M2/M5）**：①`goldens/README.md` 的「重新生成」命令块补上**先检出 pin**
  与自检，并写明只读检出的替代做法（可写克隆 + `git fetch origin <sha>` + `checkout --detach`，
  **不要 `--depth 1`**）；②三份「CI 不重生成」金样（`key` / `key_sequence` / `sound_to_char_shape`）
  的内部头部（`# reference … @ <pin>` 与来源文件 sha256）纳入校验——`key.tsv.gz` 头部为**只增不改**
  补齐（解压后 5132 条记录逐字节不变，仅新增 4 行 `#` 注释；sha 由 `e939a077…` → `7fae4983…`），
  新增 `tools/checks/verify_golden_shas.py` 并接入 CI 两个作业；③`gen_key_golden.sh` 与
  `key_probe.cpp` 补齐「输入不可读即失败 / 空输入非零退出 / 写库前 `$OUT.tmp.$$` + 至少 1 条
  `name` 与 `parse` 断言」——「入库 `key.tsv.gz` 被静默覆盖成 32 行」的路径消失。
- **文档一致性（D1–D14）**：§8 全条目加 `[✅ 已修] / [待办] / [已登记·不修+理由]` 状态前缀并与
  代码/测试逐条对齐（paging、editor 绑定、面板数字序号三条自相矛盾的登记改写；`pair_oddness`、
  `reopen_previous_selection` 两条按「已评估结案」改写）；§7 的两处守卫描述与实现对齐（并**补上**
  此前只写在文档里的 core `hux_scheme` 文本守卫、把平台黑名单升级为白名单）；已删符号残留逐处标注
  「已删 / 历史对照」（`KeyEvent::forward()` 改写为 `HUX_KEY_FORWARD_AFTER_COMMIT` + `keyEvent.forward()`）；
  §6 源内单测文件数与 §3 基线数值更新为实测；`README.md` 文档索引、4 份方案骨架 README
  （依赖方向 / 数据与 API 需求 / 契约需求）、`docs/usage.md` 手工卸载与 `lib64` 说明同步。
- **平台杂项（F5/F6/F8/F15 + M15）**：`cmake --install` 按新增的 `data/MANIFEST` 安装随包数据
  （此前只装 3 个插件文件 ⇒ 只走 CMake 会得到无词库引擎）；`install.sh` 装后逐条核对、
  `uninstall.sh` 按同一清单删除（此前 glob 装 / 枚举 7 个名字删），自检
  `tools/checks/check_data_manifest.sh` + CI `DESTDIR` 步骤；学习库运行期写入失败进状态串（F6）；
  `hux_engine_status` 指针契约改为「下一次状态刷新前有效」并在测试里钉住（F8）；
  配置页绑到无名字 keysym 时不再静默——`hotkeys:` 诊断 + C++ 壳落日志 + README 说明（F15）。

**复核整改第 5 批：遗留②③ 补金样 + cfg / 平台 / 工具清尾 ✅ 已完成**（依据
`_tmp/复核-cfg-平台.md` 的 F4/F7/F9/F10.2/F13/F14/F17、`_tmp/复核-tiger.md` 的 B7 与遗留②③、
`_tmp/复核-文档工具CI.md` 的 M3/M6/M7/M9/M11/M12；门槛：`cargo test --workspace --locked`
**341 用例 0 失败**、`tools/checks/verify_golden_shas.py` **61 项通过**、`cargo fmt --all --check` /
clippy `-D warnings` / `reuse lint`（164/164）全绿；提交 `5304bd357903` 一笔，
逐项记录见 `_tmp/批次5-收尾.md`）

- **学习 × 早提交组合金样（遗留②）**：新增 `goldens/decode_learning_evidence.tsv.gz`
  （`--early-commit 1 --required 1 --learning 1`；12257 行）+ `gen_decode_golden.lua` 的组合开关
  + 差分 `decode_learning_evidence_transcript_is_bit_exact_without_model`（>12000 条位级比对）
  + CI 两个 Lua 作业的重生成比对；覆盖 `learning=1 && truncated=1` 的截断池与 `share`/`base_share`
  双权重交互。
- **Tab 锁真机探针（遗留③）**：新增 `goldens/key_sequence_tab.tsv.gz`（8 例 / 44 步）、夹具
  `goldens/key_sequence_tab/`（`tiger_sentence.custom.yaml` 写 `tab_learning: true`）、独立生成器
  `tools/generators/gen_key_sequence_tab_golden.sh`；重放侧 `store_ready = true` 并断言
  「每个用例首次 Tab 都捕获学习基线」（`captures == 8`）。**不夸大覆盖面**：本次增量是「参照的
  `learned.store.db` 分支真的被走进 + 重放侧真的接上 `learning_selection`」，比对字段本身在
  `tab_learning` 两侧逐位相同。
- **其余清尾**：cfg F4（翻页键角色区分「角色缺失」与「显式空列表」，空列表 = 不绑定、两侧同语义）、
  F7（`hux-ffi` 字段名表 + `offset_of!` 逐字段引用 + 与 `hux_abi.h` 解析序校对）、F9（FFI 用例改用
  临时用户目录）、F10.2（角色表长度先钉住，避免空转通过）、F13（未知 / 已释放会话清粘性转发位）、
  F13.1（`paging_action` 精确 `(keycode, modifier)` 比较）、F14（平台只取不透明 mode 串）、
  F17（CI 加 `mod.rs` 守卫）；tiger B7（`page_no` 纳入逐位比对）；工具 M3（探针生成器对入库夹具
  加逐字节比对护栏，不再当副作用重写）、M6（`set_option` 前断言方案 switches 声明过该名）、
  M7（插件缺失显式报错）、M9（`gen_key_table.py` 原子写 + 干净报错）、M11（维护失败 / 用例重名入
  `check`）、M12（`key_cases.txt` 重复行加注释**保留**——删行会改动入库金样）。

**内核（hux-core）**
- `[✅ 已修]`（§8①）：契约语义边界——`OptionIds`（**已删**）/ 固定字段 `SchemeConfig` 已换成
  「方案自报角色声明 + 通用键值袋」，内核零虎码口径字段（CI 守卫 + 平台/方案用例守护）。
- `[✅ 已修]`（第 2 批 F2/F3/F11）：`host.rs` 的 `paging` 条件——`Selector::PreviousPage` 停在首页
  也**无条件**写 `paging` 标签并按参照 `Highlight(0)`；`page_cycle` 只作用于 `NextPage`
  （参照 `menu/page_down_cycle`）；上翻页判据（`kWhenPaging`）不再多带 `has_menu`/`ascii_mode` 前置。
- `[✅ 已修]`（第 2 批 F4/F5）：`Ctrl+Return`（`CommitScriptText`）按参照 `Composition::GetScriptText`
  提交脚本文本（preedit 优先去首个 `\t`，`keep_selection = true`）且不经 `Commit()`（不发提交通知）；
  `Ctrl+Shift+Return`（`CommitComment`）注释为空时只吞键（不清组合、不提交空串）。
  Shift 回退（Shift+space / Shift+BackSpace / Shift+Delete）此前已补。
- `[✅ 已修]`：`learning::reward`/`RewardNode` 与方案 `decode::learning_reward` 的重复实现已合并——
  算法只在 core 维护一份，方案把 arena 路径物化为 `RewardNode` 链后调用 core；
  同时删除 `RewardNode.text`（从未被读取的死字段），金样（learning 与 decode+learning）判定顺序正确。
- `[✅ 已修]`：核心 `session::live_caret` 与 `live_input` 判据不一致——统一为「确实带 `~` 标记」，
  `live_caret` 直接复用 `live_input` 的结果（`crates/hux-core/src/session.rs`）。
  `[待办]`（low，已登记）：方案侧 `interaction::state::{live_input, input_caret}` 仍是自持副本，
  缓冲判据取 `K_BUFFERED` 属性而非 `~` 前缀；两处由 `state.rs` 的 `save()`（写属性 + `set_buffered`）
  同步，**实测无可达差异**，故本轮只登记不改——合并要么把内核视图语义搬进方案，要么让方案委托核心，
  两者都动热路径，留待与属性层收尾一并评估。
- `[✅ 已修]`（复核整改第 4 批 D4）：`punct::pair_oddness` 已不存在——成对交替状态移入**每上下文一份**的
  `punct::PairState`（`TigerScheme` 单实例不再串台），`PunctTable` 回归只读数据（注释明写）。
  文档此前把它登记为「待办」，实为已完成。
- `[✅ 已修]`（复核整改第 4 批 D5）：`reopen_previous_selection` 的两条护栏（`status > kSelected`、
  `selected_before_editing`）经评估在本模型下**不可达**，已在 `host.rs` 就地写明理由，
  并留下「若将来引入编辑态须同步补这两道判据」的约束——按「已评估并结案」登记，不再列为待办。
- `[已登记·不修+理由]`：core 内 `std::fs` 读文件（`PunctTable::load_first`）与 §1.2「core 零平台文件 API」
  措辞的张力——口径已明确：**读取平台传入的显式路径允许**，CI 只拦 env / 时钟 / 打印 / 硬编码路径
  （§1.2 原文如此；第 4 批 C6 后这些守卫剥离注释匹配）。不再作为待办项。
- `[✅ 已修]`（第 2 批 F8–F12）：`build_valid` 同义包装删除；`PunctTable::load` 收进 `#[cfg(test)]`；
  `LearningIndex::future` 注明为「参照遗留元数据，本仓无消费者」；`Context::highlight` 对未建立菜单的段
  不写不改不通知（参照 `context.cc`）；`context_valid` 重复 `chars()` 合并为一次。
  （更早的清理：`K_HYPER_MASK`/`K_META_MASK`/`KeyEvent::caps`/`Context::has_events` 删除、
  `HostResult` 与 `KeyOutcome` 合并为 `pub use` 别名。）

- `[✅ 已修]`（复核整改第 4 批 D2）：`editor` 绑定：参照 `ExpressEditor` 表（`_tmp/librime/editor.cc` @ pin `33e78140`）为
  `{Return,0}→CommitRawInput`（✅ 已实现）、`{Return,Ctrl}→CommitScriptText`、
  `{Return,Ctrl+Shift}→CommitComment`、`{BackSpace,0}→RevertLastEdit`、`{BackSpace,Ctrl}→BackToPreviousSyllable`、
  `{Delete,0}→DeleteChar`、`{Delete,Ctrl}→DeleteCandidate`、`{Escape,0}→CancelComposition`，
  另有 `FallbackOptions::All` 的 Shift 回退（✅ 已补 Shift+space / Shift+BackSpace / Shift+Delete）。
  ✅ 已补并已按参照核对语义（第 2 批 F4/F5）：`Ctrl+Return` → `CommitScriptText`
  （`Context::GetScriptText()` → `Composition::GetScriptText(keep_selection = true)`：确认段取候选文字，
  否则候选 preedit 去首个 `\t`，否则原始输入切片；`sink()` 后 `Clear()`，**不**触发提交通知）、
  `Ctrl+Shift+Return` → `CommitComment`（仅注释非空时提交并清空）。
  **踩坑记录**：模式里的 `K_A | K_B` 是**或模式**而非按位或，组合修饰键必须写成
  `(code, modifier) if modifier == K_A | K_B`；本轮该 bug 已修，并全仓 grep 确认无同类写法。
- `[✅ 已修]`：覆盖缺口已补——宿主链绑定（金样走不到：真机路径上 `Return`/`space`/`Escape` 等先在方案
  `processor` 被消费）现有单测钉住：Confirm / Cancel / `Ctrl+BackSpace` / `Ctrl+Return` /
  `Ctrl+Shift+Return` / Shift 回退；`CancelComposition` 按参照 `ClearPreviousSegment() || Clear()` 断言。
- `[✅ 已修]`（复核整改第 4 批 D1）：翻页键条件按参照（`key_binder.cc:262`）严格化——下翻页 `when: has_menu`、
  上翻页 `when: paging`（`mark_paging` 于翻页时写入标签）；未翻页时上翻页键**不消费**，
  交后续处理器落作标点（参照行为）。core 与平台两侧的单测按新契约重写，
  `docs/config.md`、`docs/rust-migration.md` 的翻页键描述同步。
  （第 2 批 F2 补齐了这条契约的**首页**分支：`Page_Up` 停在首页同样写标签，故紧随其后的 `-` 成立。）

**工具 / CI**（状态前缀同上）
- `[✅ 已修]`：**三个**探针生成器（`key` / `key_sequence` / `sound_to_char_shape`）写库前先写
  `$OUT.tmp.$$` 并断言记录非空，再原子 `mv`。第 4 批 M5 补齐了此前漏掉的 `key`
  （`gen_key_golden.sh` 断言至少 1 条 `name` 与 1 条 `parse`，且两个输入文件不可读时
  `key_probe` 返回非零）——这正是「入库 `key.tsv.gz` 可被静默覆盖成 32 行」的路径；
  负向对照（旧探针 32 行 / exit 0 ⇒ 新探针 exit 2、写库断言拦下）见 `_tmp/批次4-文档工具CI.md`。
- `[✅ 已修]`：`gen_pinyin_index.py --check` 与**由 `--source` 重建**的内容逐字节比对
  （此前不带 `--manifest` 时对任意文件都打印 `check ok`，属恒真检查）；显式传
  `--manifest` 而文件缺失即 `exit 1`。正负例均已验证。
- `[✅ 已修]`（第 4 批 M1/M2）：`goldens/README.md` 的「重新生成」命令块补上**先检出 pin**
  （`git -C "$REF" checkout --detach abad411…` + `test "$(git … rev-parse HEAD)" = …` 自检）并写明
  参照检出只读时的替代做法（工作区内可写克隆、`git fetch origin <sha>` + `checkout --detach`，
  **不要 `--depth 1`**——浅克隆会让需要本地合并的历史操作被判「无关历史」）；
  新增 `tools/checks/verify_golden_shas.py`（README sha 表逐行比对 + 三份金样内部头部 pin/sha
  交叉核对 + `--reference` 时按 pin `git show` 核参照文件），接入 CI 的 `rust` 与 `golden` 两个作业。
  负向对照（改坏一处 sha / 改坏头部 pin / 新增未登记金样 / 参照检出 sha 不符）全部报错退出。
- `[✅ 已修]`（第 4 批 C6/C7）：源码文本守卫改为**剥离注释后匹配**
  （`tools/checks/rust_source_grep.py`）——修正「注释也被拦」与 §7 声明相反的问题；
  并补上 §7 声称存在、实际缺失的 core `hux_scheme` 文本守卫，把平台「只允许 3 个路径」
  的黑名单升级为**模块白名单 + 导入名白名单**（守卫只增不减，负向对照见批次记录）。
- `[✅ 已修]`：CI 加 `--locked`、`timeout-minutes: 30`（五个作业）与 `concurrency`（同分支取消旧运行）。
  **有意不加** 缓存 action（保持第三方依赖面最小，冷编译代价可接受）。
- `[✅ 已修]`：`addon` 作业加装 `librime-dev` 并对 `tools/probes/*.cpp` 做 **`g++ -fsyntax-only`**
  语法检查（不链接、不运行），防止探针长期无人编译而静默腐坏。本机已实测两个探针语法通过。
- `[待办]`（第 4 批 C8，供应链钉版本；本轮只登记不实施，因为无法在本机验证 GitHub 侧可用性）：
  ① `uses:` 的移动标签（`actions/checkout@v4`、`fsfe/reuse-action@v6`、`dtolnay/rust-toolchain@stable`）
  改为钉 commit sha（建议开 Dependabot 的 `github-actions` 生态自动更新）；
  ② 新增 `rust-toolchain.toml`（`channel` 钉到本机验证版本 `1.98.1` + `components = ["rustfmt", "clippy"]`，
  同时给 `dtolnay/rust-toolchain` 传 `toolchain:` 输入），避免 stable 漂移让 `cargo fmt --check` /
  clippy `-D warnings` 无预警变红；
  ③ `archlinux:latest` 是 `golden-lua-latest` 作业的**目的**（测最新 Lua），故不钉镜像；已把
  `pacman -Sy` 改 `-Syu`（Arch 不推荐部分升级）。③ 已实施，①②待办。

### 复核整改总账：四份只读审计逐条归宿

> 四份只读审计报告的原件在 `_tmp/复核-{core,tiger,cfg-平台,文档工具CI}.md`（`_tmp/` 已 gitignore、
> 会被清理）——**本小节即它们的归宿**：每条发现一行，`[✅ 已修]` 一律指到本仓提交或文件级守护，
> 不依赖再读原件。编号沿用报告原编号（core `F1–F13`、tiger `A1–A8 / B1–B8 / C1–C8`、
> cfg-平台 `F1–F17`、文档工具CI `D1–D14 / M1–M17`）。
>
> **计数**：发现 **85 条**（core 13 / tiger 24 / cfg-平台 17 / 文档工具CI 31；tiger 报告摘要记 23 条，
> 其中 `C8` 被单列为「信息·非缺陷」，此处按编号全列 24 条），拆成 **96 行**
> （11 个独立子项行：core `F13.1–13.3`、tiger `B2 子断言` 与 `C3`/`C7` 的内部 / 跨 crate 拆分、
> cfg `F10.1–10.3` 与 `F15.1–15.5`）。**状态分布：`[✅ 已修]` 84 / `[待办]` 6 /
> `[已登记·不修+理由]` 5 / `[误报·已核实]` 1。**（其中 6 条一档项由本轮「① 立刻做」收口：
> cfg `F11`、`F15.2`–`F15.4`、`F10.3`、文档 `M4`；剩余 6 条 `[待办]` 见各表。）
>
> **批次与提交**：第 1 批 `8336316a`（角色守护）/`a640141d`（`unframe` 崩溃面）/`80c965a2`（标识符中性化）；
> 第 2 批 `a792e79d`（core F2–F12）+ `d94face3`（金样 +6 例）；第 3a 批 `56fca679`；
> 第 3b 批 `f6372f37`/`63b8d74e`/`84e3beba`/`ff9217e1`；第 4 批 `37f27882`/`9774b639`/`d2f771cb`/`17f7bf2a`；
> 第 5 批 `5304bd35`。逐批「修法 → 守护 → 负向对照」见 `_tmp/批次{1,2,3a,3b,4,5}-*.md`。

#### 总账 · hux-core（`_tmp/复核-core.md`）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| F1 | `learning::unframe` 对「长度合法但落点非字符边界」的值 panic（签名却承诺 `Option`），平台读库在 `extern "C"` 中 ⇒ 开库即 abort | [✅ 已修] | 第 1 批 `a640141d`：改 `value.get(a..b)?`；平台坏帧跳过并计入 `skipped N undecodable record(s)` 诊断（`hux_engine_status` 可见、库仍可用）。守护：core `unframe_rejects_non_char_boundary_slices`、平台 `learning_store_skips_undecodable_records_with_diagnostic` |
| F2 | `Page_Up` 停在首页不写 `paging` 标签 ⇒ 紧随的上翻页键被标点分支吞掉并误提交组合 | [✅ 已修] | 第 2 批 `a792e79d`：按 `selector.cc` 无条件写标签 + `Highlight(0)`；core `page_up_at_first_page_arms_the_paging_binding`、平台 `menu_paging_keys_are_not_shadowed_by_the_punctuation_branch` ⑥、金样新例 `nav_page_up_home_reset`（`d94face3`） |
| F3 | `page_cycle` 的「首页向上翻到末页」参照没有（未登记偏离） | [✅ 已修] | 第 2 批：删除该分支（参照 `menu/page_down_cycle` 只读于 `NextPage`），doc 与 `docs/config.md`、`docs/rust-migration.md` 同步；单测 `selector_page_cycle_wraps_next_page_only` |
| F4 | `Ctrl+Return` 语义错：提交候选文字并**触发提交通知（写学习库）**，参照提交脚本文本且不经 `Commit()` | [✅ 已修] | 第 2 批：新增 `Composition::script_text` / `Context::get_script_text`（preedit 去首个 `\t` 优先、`keep_selection = true`），`direct_commit + clear`、移除 `commit_notifier`；§8「`GetScriptText` 待复核」就此结案；金样 `editor_ctrl_return` |
| F5 | `Ctrl+Shift+Return` 注释为空 / 无候选时参照不清组合，本实现清空并提交空串 | [✅ 已修] | 第 2 批：仅注释非空才提交并清空；单测 + 金样 `editor_ctrl_shift_return` |
| F6 | `punctuator` 的「净效果等价」只在标点落在输入末尾成立（caret 居中时参照在 caret 处插入且**不提交**） | [✅ 已修] | 第 2 批：按 `Punctuator::ProcessKeyEvent` + `ConcreteEngine::Compose`（`active_input = input[..caret]`）实现；单测 + 金样 `punct_mid_caret_comma` / `punct_mid_caret_no_menu` |
| F7 | 配置袋**没有错误通道**：缺角色 / 类型不符 / 角色名拼错一律静默 `None` / 空切片 | [✅ 已修] | 第 1 批先加「未识别 / 缺少角色」诊断；第 2 批补 `ConfigError{Missing,TypeMismatch}` + `require_{bool,count,text,texts}` + `texts -> Option<&[String]>` + `apply_config -> Result<(), Vec<ConfigError>>`，平台并入 `config:` 状态串；平台用例 `scheme_config_diagnostics_reach_the_status_string` |
| F8 | `build_valid` 是 `event_valid` 的同义包装 | [✅ 已修] | 第 2 批：删除 `build_valid`，`build` 直接用 `event_valid`（学习差分 oracle 不变） |
| F9 | `LearningIndex::future` 只写不读 | [✅ 已修] | 第 2 批：按审计第二方案保留 + doc 注明「参照遗留元数据，本仓无消费者」（删字段会与参照 `M.runtime_index` 不同构） |
| F10 | 死 pub 面：`PunctTable::load`、`Value::Text` / `SchemeConfig::text` | [✅ 已修] | 第 2 批：`PunctTable::load` 收进 `#[cfg(test)]`；`Value::Text` / `text` 保留为 `require_text` 的活契约并注明（审计本身判「通用容器词汇、不算违规」） |
| F11 | `paging_action` 对 `when: paging` 的前置比参照严格（多带 `ascii_mode` / `has_menu`） | [✅ 已修] | 第 2 批：Up 分支只判「命中 `page_up_keys` + 末段标签」，Down 分支保留 `menu_available`；单测加「`ascii_mode` + 标签 ⇒ 仍判 Up」断言 |
| F12 | `Context::highlight` 在段未翻译时仍改写 `selected_index` 并推 `Update` | [✅ 已修] | 第 2 批：`if !segment.translated { return false; }`；单测 `highlight_skips_untranslated_segment_without_update` |
| F13.1 | `paging_action` / `key_binder` 用 `repr()` 字符串比较代替参照的 `(keycode, modifier)` 精确查表 | [✅ 已修] | 第 5 批 `5304bd35`：改精确比较（`K_MODIFIER_MASK` 内无名位不再碰撞、每键少一次分配） |
| F13.2 | `context_valid` 对同一串调用两次 `chars()` | [✅ 已修] | 第 2 批顺带合并为一次 |
| F13.3 | `Segment::selected_index: usize` 无法表达参照的 `-1`（无选择）态，`Ctrl+Delete` 退化为吞键 | [待办] | 需先定「是否补 `DeleteCandidate` 通道」；若要补，前置是 `selected_index: Option<usize>`（或显式 `has_selection`）+ host 单测 + 探针用例，成本中等。**待用户确认是否需要该交互**；现状 `host.rs` 已写明「本实现无删除通道」 |

#### 总账 · hux-scheme/tiger（`_tmp/复核-tiger.md`）

| 编号 | 一句话问题 | 状态 | 归宿 |
|---|---|---|---|
| A1 | 反查段数字直选索引口径与上游分叉（页相对 vs 绝对）且零覆盖 | [✅ 已修] | 第 3a 批 `56fca679`：删除 addon 的「页相对」分支，改上游 `index = digit - 1`（越界惰性消费）；补反查段翻页后按数字的用例 |
| A2 | 音反查索引（TCSRV01）解析对畸形文件无边界 / 溢出校验 ⇒ panic，且一致性校验可被回绕绕过 | [✅ 已修] | 第 3a 批：音节 / 拼写 / 组码 id 上界校验、`checked_add` 逐组「不超过 entry 总数」、按最小条目字节推容量（拒绝超大头部）、截断检测，失败返回诊断；6 个畸形字节用例 + 1 个良构对照 |
| A3 | 上下文属性层在生产路径「只写不读」，legacy 迁移与属性解析永不可达（注释与代码不符） | [✅ 已修] | 第 3b 批 `63b8d74e`：按审计选项②删除 `load` / `read_locks` / `parse_committed_property` / `save_locks` + `K_LOCKS` / `K_COMMITTED*` / 4 个 legacy 键 + `legacy_cleared`，属性层只留宿主与内核共享的 `K_BUFFERED`；`interaction.rs` 注释同步 |
| A4 | 音反查段的撇号音节分隔**未实现**，注释声称与代码不符 | [✅ 已修] | 第 3a 批：识别模式与注释改为与 schema 的 `speller/delimiter` 一致（撇号只放行、**不切分**）；第 3b 批 `ff9217e1` 把「与未修复 librime 对齐 + 回归做法」登记进 §8 偏离③末条。切分依赖上游 [rime/librime#1233]，本机 1.17.0 未含 ⇒ 与已入库金样（含撇号段 0 候选）一致 |
| A5 | `MobileModel::load` 不校验 `limits`（与 `configure_cache` 口径不一，可 `assert` panic） | [✅ 已修] | 第 3b 批 `84e3beba`：抽出 `validate_limits` 供两条入口共用（非法上限返回错误）；单测 `load_rejects_invalid_cache_limits` |
| A6 | 32 位目标的 `u64 -> usize` 截断 / `usize` 溢出（android 属优先平台） | [✅ 已修] | 第 3b 批：`tri_ctx_count` 显式 `usize::try_from` + 「上下文数 ≤ 文件可容纳条目数」校验、`uni_count * 8` 改 `checked_mul`、索引字节数全程 `u64`；单测 `implausible_trigram_context_count_is_rejected` |
| A7 | NaN 语义与 Lua 相反（正常数据不可达） | [已登记·不修+理由] | 第 3b 批：`reward_for_weight` 的 `clamp` 与 `logp` 的 `max` 两处**只注明差异**——入口 `weight > 0.0` 已排除 NaN ⇒ 正常数据不可达、无金样支撑，改行为属投机 |
| A8 | `try_commit_mature_prefix` 平局时的选择顺序（参照依赖 Lua 哈希序） | [✅ 已修] | 第 3b 批 `63b8d74e`：`tracker_better` 三元全等时加 `text` 字典序兜底（与「按 key 排序取首个更优」一致、判据自身反对称）；单测双向断言（`乙` vs `甲`） |
| B1 | 金样重放未按**出厂缺省**开 `digit_select`，差分层与出厂默认路径脱节 | [✅ 已修] | 第 3a 批：重放显式 `set_option(OPTION_DIGIT_SELECT, true)`（出厂缺省口径）并把「上游并入编码 / 本仓直选提交」登记为 `AddonExtension`（`digit_menu_select`） |
| B2 | 分段常量取自 `92a0b54`（`delimiter: " '"`），与主金样 pin `abad411`（`" "`）不一致 ⇒ 未登记的行为差异 + 零覆盖 | [✅ 已修] | 第 3b 批 `f6372f37` + `ff9217e1`：保留 `" '"`（探针以 `PIN=92a0b54` 重跑同一用例与本仓**逐位相同** ⇒ 差异是上游自己的后续提交）、金样**只增** 2 例（`apostrophe_*_page`）、登记 `BranchPinDelimiter`；负向对照：改回 `" "` 即报「登记的偏离已消失」 |
| B2·子断言 | 审计称「本仓把 `ab'1` 切成 abc 段 + raw 段 ⇒ 有 rank-3 候选」 | [误报·已核实] | 第 3b 批实测**两 pin 行完全一致**（`ab'1` 都是 `count=0`）：段结构差异不落在比对面（`preedit` 按设计不比对）；`apostrophe_*_split` 两例逐位通过、无需登记 |
| B3 | `DEVIATED_CASES` 三层自校验**不能证明「登记即偏离」**（一层构造性恒真）⇒ 可静默取消任意用例覆盖、回退成上游行为也不失败（唯一 high） | [✅ 已修] | 第 3a 批：升级为**期望值表** `DEVIATIONS`（登记 = 金样名 + 本仓逐步期望值），断言「期望 ≠ 金样」的步集合 == 「实测 ≠ 金样」的步集合、登记名唯一且真实存在、登记项必须确有差异；反向：实现回退成上游行为即报「登记的偏离已消失」 |
| B4 | `key_sequence` / `sound_to_char_shape` 差分缺覆盖下限断言（金样被截断会静默通过） | [✅ 已修] | 第 3b 批：加下限（`key_sequence` ≥68 例 / ≥285 步、重放面 ≥63 例 / ≥265 步；`sound_to_char_shape` ≥31 例 / ≥164 步、重放面 ≥28 例 / ≥149 步）；负向对照：截断金样 ⇒ 「用例数不足」失败 |
| B5 | ngram「真实模型」差分在 CI 恒静默跳过（输出仍是 `ok`） | [✅ 已修] | 第 3b 批按审计第二方案落地 `HUX_REQUIRE_SAMPLE=1` 闸门（缺失即**失败**，缺省仍跳过以保住无模型环境全绿）。**残留**：CI 未加可选作业——需 ~448 MiB 模型下载 + 缓存 action，与「有意不加缓存 action」的既有取舍冲突，成本已登记、待人决定 |
| B6 | 融合事件的**产出与接受**两侧在 tiger crate 内零用例 | [✅ 已修] | 第 3b 批 `63b8d74e`：2 条端到端用例（Direct 胜 / Composed 胜，走真实菜单 + 数字直选）+ 4 条 `learning_glue` 级用例；顺带修掉真缺陷——`select_candidate_at` 重复暂存使同一次点击写两条成对偏好 |
| B7 | `key_sequence` 金样的 `page_no` 列既不比对也未登记 | [✅ 已修] | 第 5 批 `5304bd35`：`page_no`（`fields[9]`）纳入逐位比对（`selected_index / page_size`，无菜单时为 0），模块注释同步 |
| B8 | 偏离跳过粒度是**整例**（18 步里 10 步非偏离步也不比对） | [✅ 已修] | 第 3a 批：期望值表令登记例**每一步**都比对（非偏离步对金样、偏离步对登记期望），整例 skip 面消失；原「边际损失 ≈0」的结论保留为旁证 |
| C1 | 无使用者的 `pub` API（11 项） | [✅ 已修] | 第 3b 批：删 10 项（`MobileModel` 7 个访问器、`SoundToCharShapeIndex::group_count`、`K_OPTIONS_ERROR`、`Decoder::{clear_learning, learning_index_mut}`），`lexicon::candidate_paths` 收进 `#[cfg(test)]`。残留：`pub` 面整体仍偏大（报告统计 243 个公开声明，多数仅测试 / 本文件用），未收窄 |
| C2 | 只写不读字段（`Lexicon::{codes_path,ranks_path,whitelist_path}`、`Supplement::path`、`LexicalModel::path`、`MobileModel::{path,source_index_bytes}`、`SoundToCharShapeIndex::syllables`、`Evaluated::lexical_score`） | [✅ 已修] | 第 3b 批：删除清单全部落地（`syllables` 保留为诊断面并注明「唯一读取方是测试」）。残留：`Evidence::raw_lengths` / `DecodeOutput::completed_truncated` 属金样观测面，审计建议「宜注明」——**未加注释**（建议下一批补一行 doc，零风险） |
| C3（crate 内） | 重复常量 / 重复实现（漂移风险） | [✅ 已修] | 第 3b 批：`candidate_limit` 三份 → `decode` 一份（其余 `pub use`）、`early_commit_minimum_share` 两份 → 一份、`ngram_bench` 的私有 hex 解码改用 `hux_test_support::decode_hex` |
| C3（跨 crate） | `lexicon::candidate_paths` ↔ `hux_core::scheme::asset_paths`（逐字同逻辑）、`state::{live_input,input_caret}` ↔ `core::session::{live_input,live_caret}`（同构双份）、两套 `BOS/EOS`（`&str` vs `char`） | [已登记·不修+理由] | 第 3b 批**只报告不合并**：三者都牵动契约面或热路径类型（core 版已有平台调用者），合并需单独排期；现状无行为漂移（判据已统一），风险是后人改一侧忘另一侧 |
| C4 | 压警告与死赋值（`decode_bench` 的 `let _ = LEXICAL_FILE;`、`sound_to_char_shape` 的 `farthest` 死赋值与不可达 `else`） | [✅ 已修] | 第 3b 批：删死赋值与多余 import，不可达分支就地加说明 |
| C5 | `Chunk.cursor` 恒为 0（构造处字面量 0，仅读一次当种子） | [✅ 已修] | 第 3b 批：去掉字段（`vec![0; n]`） |
| C6 | 公开 API 暴露 `hashbrown::{HashMap, HashSet}`（跨 crate 复用需匹配 `hashbrown` 版本） | [待办] | 未改。两条路：① 公开签名换 `std::collections`（牺牲统一哈希实现 / 性能，需基准数据）；② 保留并写明「公开 API 与 `hashbrown` 版本绑定」。成本低，**需一次口径决策**（`lexicon.rs` 的 `parse_ranks_content` / `parse_whitelist_content` / `Supplement::build` / `parse_supplement_content`，`lexical.rs::score_with_cache`） |
| C7（读音串） | 音反查索引首次加载的无效分配（为每个组构造 `Vec<&str>` + `String`） | [✅ 已修] | 第 3b 批：读音串改按需构造（`Option<String>` 惰性）——真实索引 600,869 组里仅 412 组含单字词条；实测首次加载 96–99 ms → **81–88 ms**（release，各 3 次） |
| C7（`Group.code`） | 60 万次 `Group.code: Vec<u16>` 小分配 | [待办] | 未做（第 3b 批登记）：需先有基准数据，且要改组查找 / 前缀剪枝 / `collect_chunks` 的取值路径（扁平 `Vec<u16>` + `(start, len)`），收益与风险不匹配，留待性能批 |
| C8 | 信息项：NaN 语义（见 A7）、`build_edges` 每位置线性扫全部拼写键（449 键 × 段长） | [已登记·不修+理由] | **非缺陷**：NaN 已按 A7 注明；449 键量级的线性扫经评估可接受，报告本身判「仅记录」 |

> 报告「二、已知遗留项逐条结论」的 6 条不在 A/B/C 编号内：① 融合子串过滤无专用用例 → 已闭合（B6 行）；
> ④ `SEGMENTATION_DELIMITER` → 已闭合（B2 行）；⑤ `learning_potential`、⑥ root `learning_score`
> → **仍成立但为空操作**（两侧同构，仅当将来给 Direct 边 / root 写非零值时才会分叉）；
> ② 组合金样、③ Tab 锁探针见上文 3b 段落（**第 5 批已闭合**）。

#### 总账 · hux-cfg / hux-ffi / platform（`_tmp/复核-cfg-平台.md`）

| 编号 | 一句话问题 | 状态 | 归宿 |
|---|---|---|---|
| F1 | 配置袋角色名「双写字面量」：漂移后**全绿**且设置静默失效（实测兜底率 7/9） | [✅ 已修] | 第 1 批 `8336316a`：方案自报 `SCHEME_CONFIG_ROLES` + `TigerScheme::load` 报「未识别 / 缺少角色」诊断（进状态串）；平台 `scheme_config_roles_match_the_scheme` 逐项比对 + 单侧改名负例；cfg `role_tables_stay_disjoint_and_ordered` |
| F2 | C++ 状态菜单角色序与 `hux_abi.h` 零绑定，`kLabels[role]` 存在越界读（UB） | [✅ 已修] | 第 1 批：`hux_abi.h` 加哨兵 `HUX_OPTION_COUNT` + C++ `static_assert(std::size(kLabels) == HUX_OPTION_COUNT)` + Rust 用例 `option_role_order_matches_the_abi_header`（解析头文件枚举序 / 个数 / 下标连续）。负向对照：加第 6 角色 ⇒ C++ 编译失败；调序 ⇒ Rust 用例失败 |
| F3 | 学习库读入路径可 panic，且发生在 `extern "C"` 中 ⇒ 进程 abort | [✅ 已修] | 第 1 批 `a640141d`（与 core F1 同一处修复）：`unframe` 改 `value.get(a..b)?`，平台坏帧跳过 + `skipped` 诊断 |
| F4 | 「空翻页键列表」语义两处实现不一致，且单测钉住的是**生产不走**的那条 | [✅ 已修] | 第 5 批 `5304bd35`：契约定为「角色**显式给出即以此为准**，空列表 = 不绑定」，方案侧 `page_up_keys: Option<Vec<String>>` + `optional_texts`；两侧用例 `empty_page_key_lists_unbind_the_keys`（方案）/ cfg 单测同步 |
| F5 | `cmake --install` 装不到数据文件 ⇒ 只用 CMake 安装得到「无词库」引擎 | [✅ 已修] | 第 4 批 `37f27882`：新增 `data/MANIFEST` 作为**装 / 卸 / CMake 三处唯一清单** + `tools/checks/check_data_manifest.sh`（接入 CI）+ CI `DESTDIR` 步骤对齐；负向对照 5 组 |
| F6 | 学习库写入失败被吞（`error` 只在构造时读一次） | [✅ 已修] | 第 4 批：`finish()` 后 `observe_learning_error()`，变化即刷 `; learning: <error>` 状态串；平台用例 `learning_write_failure_reaches_the_status_string` |
| F7 | ABI 结构体三方对应缺「跨语言字段级」断言：同宽字段换序无法发现 | [✅ 已修] | 第 5 批 `5304bd35`：`hux-ffi` 增字段名表，`c_layout_matches_header` 以 `offset_of!` **逐字段引用**（改 Rust 字段名即编译失败）并新增 `options_field_names_match_header_order` 与 `hux_abi.h` 解析结果校对顺序；附注的「11 个标量」陈旧注释同批改（见 F15.1） |
| F8 | `hux_engine_status` 的指针有效期与头文件承诺不符 | [✅ 已修] | 第 4 批：契约改为「**直到下一次状态刷新前有效**」（`hux_abi.h` + `abi.rs` + `platform/fcitx5/README.md`）；平台用例 `status_pointer_must_be_read_again_after_a_refresh` |
| F9 | 三处测试污染真实用户目录（`hux_engine_new(nullptr)` 走生产构造并在 `~/.local/share/…` 开学习库） | [✅ 已修] | 第 5 批：改用 `ffi_engine(temp_user_dir(…))`（`Engine::new_with_dirs` 注入临时用户目录），并断言临时目录里确实建起学习库（不再触碰真实 `~/.local/share/fcitx5/hux`） |
| F10.1 | `engine_applies_learning_after_key` 只断言 `store_ready` 与 mode 前缀（删掉 `apply_learning_index` 仍全绿） | [待办] | 未改（现注释自认「只验证接线」）。需补一条能观察到「索引已应用」的断言（学习库版本 / 方案内部诊断）；成本低，宜与 F10.3 同批做 |
| F10.2 | `runtime_option_roundtrip_and_whitelist` 在 `runtime_options()` 返回空列表时**空转通过** | [✅ 已修] | 第 5 批：循环前先 `assert_eq!(roles.len(), hux_cfg::roles::RUNTIME_OPTION_ROLES.len())` |
| F10.3 | `schema_defaults_match_settings_defaults` 是**单向**的（新增 `Settings` 字段而无 `.path{}` 不会失败） | [✅ 已修] | 本次：新增测试 `every_settings_field_is_declared_in_the_schema`——`(字段, schema 路径, offset_of!(Settings, 字段))` 三列表（改字段名即**编译失败**）+ 字段数钉 17 + `Settings` 字段集与 `hux.cpp` 路径集**双向相等**（宿主显示项 `PanelPreedit` 单独登记）。负向对照实测：把路径改成 `PageSizeX` ⇒ 报「`Settings::page_size` 未在 shell/hux.cpp 的 schema 中声明」；还原即通过 |
| F11 | 角色解析失败时「全有或全无」，与 §8① 措辞（「该角色不接线」）不符 | [✅ 已修] | 本次：§8① 第 4 步措辞改为**全有或全无**（`OptionKeys::resolve` 任一问题即 `Err` ⇒ 平台不接线**任何**角色），并同步 `engine.rs::resolve_option_roles` 的文档注释 |
| F12 | 四张角色清单互相之间的包含关系无断言 | [✅ 已修] | 第 1 批：`runtime_role_tables_cover_the_declared_roles`（`SCHEME_OPTION_ROLES` ⊆ `RUNTIME_OPTION_ROLES`、`store_defaults` ≡ 运行时角色、`option_defaults` ⊇ 运行时角色 + `ascii_punct`）+ `role_tables_stay_disjoint_and_ordered` |
| F13 | `forward_after_commit` 是粘性输出标志，会话缺失时不清零（返回无 `CONSUMED` 的转发位） | [✅ 已修] | 第 5 批：`Engine::key` / `select_candidate` 在 `with_session` 返回 `None` 时显式清位；平台用例 `unknown_session_does_not_reuse_the_sticky_forward_flag`（未知 / 已释放 / 候选点击三条） |
| F14 | 平台用例钉住了「不透明」mode 串的字面格式 | [✅ 已修] | 第 5 批：平台只断言「非空 / 随配置变化」，串格式（含 `dup=0`）归方案用例 `learning_mode_follows_config_and_rules` |
| F15.1 | `crates/hux-ffi/src/lib.rs` 注释「11 个标量 int32」与 13 个不符 | [✅ 已修] | 第 5 批：改为「13 个标量 int32 + 4 个键位列表」并注明本次修正（`lib.rs:175`） |
| F15.2 | `conf/hux.addon.conf` 注释「动态库名（不含前缀 / 后缀）」与值 `Library=libhux` 矛盾（**值对、注释错**） | [✅ 已修] | 本次：`conf/hux.addon.conf` 注释改为「**含 `lib` 前缀**、不含 `.so` 后缀；与系统其它 `SharedLibrary` addon 写法一致」（值本就正确） |
| F15.3 | `engine.rs` 注释「无存储时运行时开关以现存会话为模板」与代码不符（有存储时也执行）+ `HashMap::values().next()` 迭代序的隐性假设 | [✅ 已修] | 本次：`engine.rs` 注释改写——该块与**是否存在存储无关**（`store.covers` 门只作用于设置项）；取 `values().next()` 是任一会话，因运行时开关同引擎内恒等故结果幂等；并注明「若将来运行时开关可**按会话分叉**，此处必须换成显式单一来源」 |
| F15.4 | `store.rs` 模块注释声明 legacy 回退「缺失键 → `user.yaml` 的 `var/option/<name>`」，但 `sync` 只遍历 `store_defaults` ⇒ `ascii_punct` 被读进 `values` 却**永不生效**（死读） | [✅ 已修] | 本次：选择**文档化**而非把 `ascii_punct` 纳入 `store_defaults`（后者会改变持久化语义）——`store.rs` 模块注释写明「回退只对已声明缺省生效；未纳入 `store_defaults` 的键（如宿主自持的 `ascii_punct`）读入 `values` 但不生效；将来要让某键走持久化须先加入该表」 |
| F15.5 | 配置页绑到**无名字 keysym**（媒体键等）时该绑定静默消失、无诊断 | [✅ 已修] | 第 4 批：`Engine::apply_settings` 经 `unparsable_key_bindings` 把 `hotkeys: 忽略无法识别的绑定 <角色>=<键名>` 写进状态串，C++ 壳落 `FCITX_INFO`；用例 `unparsable_hotkey_binding_reaches_the_status_string` |
| F16 | C++ 壳两处脆弱模式：`applyUpdate` 每次 UI 刷新都重建状态区；`HuxCandidateWord::select` 内同步触发回调可能销毁候选对象自身 | [待办] | 未改（当前**无实测故障**，C++ 侧以 `session == nullptr` 早退规避）：需真机 fcitx5 压力验证后再定是否投递到事件循环；本机无 fcitx5 运行环境 |
| F17 | 纪律项：全仓无 `mod.rs` 但 CI 无守卫；分层守卫只扫 `platform/fcitx5/src`（不含 `shell/`） | [✅ 已修] | 第 5 批：CI `Layer dependencies` 步加 `find crates platform -name mod.rs` 断言为空；`shell/` 不扫经复核**无实质风险**（C++ 无法引用 Rust 模块），按「已核实」保留现状 |

> 报告 §2 的**口径名逐条清单**（A1–A11 / B1–B8 / C1–C5，不计入 F1–F17 的 17 条发现）归宿：
> (b) 全部**标识符**已中性化（第 1 批 `80c965a2`，含 `ROLE_*` 常量名、`Settings` 字段、`hux_options`
> 成员、C++ 成员名；映射表见 `_tmp/批次1-守护与中性化.md`）；(a) **线上字符串全部保留**——
> `tiger_sentence.options.yaml`、`tiger_sentence_options_error`、与上游同名的三个选项键 + hux 扩展
> `tiger_sentence_digit_select`、学习库目录名 `tiger_sentence_learning_<hash>`、18 个 `.path{}` 键、
> `Icon=fcitx-tiger`、`Library=libhux`（本轮口径 C 明确「只改标识符」，改值会破坏上游互通 / 老用户配置 /
> ABI 布局）；(c) 注释与文档保留（参照出处）。**残留**：选项键字面量的第四份副本仍在
> `platform/fcitx5/src/tests.rs`（未收敛为单一测试常量）；`A10` 的注释错误见 F15.2。
>
> 报告 §5「未复现 / 需他人确认」：`HuxSession` 的析构契约依赖 fcitx5
> `InputContextPropertyFactory::unregister()` 会销毁已存在的属性——**仍未确认**（本机无 fcitx5 源码，
> 需在有源码的环境核一次）；`HuxCandidateWord::select` 期间自毁未做真机压力验证（见 F16 行）；
> C++ 侧编译 / 安装实测已由 CI `addon` 作业覆盖（第 4 批本机复跑）。

#### 总账 · 文档 / 工具 / 金样机制 / CI（`_tmp/复核-文档工具CI.md`）

| 编号 | 一句话问题 | 状态 | 归宿 |
|---|---|---|---|
| D1 | §8「`paging` 条件未实现」与同文档 ✅ 标记直接矛盾 | [✅ 已修] | 第 4 批 `17f7bf2a`：改标 `[✅ 已修]`（第 2 批 F2/F3/F11）并指向偏离章节 |
| D2 | §8「`editor` 未实现 `FallbackOptions::All` / `Ctrl(+Shift)+Return`」与 ✅ 矛盾 | [✅ 已修] | 第 4 批：改标 ✅（第 2 批 F4/F5 已补语义），`GetScriptText` 子项已结案 |
| D3 | §8「面板数字序号…仍显示序号」与 ✅ 矛盾 | [✅ 已修] | 第 4 批：删去序号半句；**页大小**改 `[已登记·不修+理由]`（核心页大小是构造期参数，`docs/config.md` 未承诺运行时可改） |
| D4 | `punct::pair_oddness` 已不存在（条目过期） | [✅ 已修] | 第 4 批：改标 ✅——成对状态入每上下文一份的 `punct::PairState`，全仓 `pair_oddness` 0 命中 |
| D5 | `reopen_previous_selection` 两条护栏已按「不可达」结案 | [✅ 已修] | 第 4 批：按「已评估并结案」登记（`host.rs` 就地写明理由与将来约束） |
| D6 | `K_HYPER_MASK` / `K_META_MASK` / `KeyEvent::caps` / `Context::has_events` 条目过期 | [✅ 已修] | 第 4 批：移入「更早的清理」✅（全仓 0 命中） |
| D7 | `docs/rust-migration.md` 引用**不存在**的 API `KeyEvent::forward()` | [✅ 已修] | 第 4 批：改写为「C ABI 处置位 `HUX_KEY_FORWARD_AFTER_COMMIT` + C++ 壳据 `keyEvent.forward()` 决定重发」，并注明该 API 从未存在 |
| D8 | §6 源内单测文件计数过期（21 → 24） | [✅ 已修] | 第 4 批：更新为 24（内核 7 / 方案 8 / 配置 4 / 助手 1 / 平台 3 / ffi 1）+ 2 个 `tests.rs`，并写明数法 `grep -rl '#\[cfg(test)\]'` |
| D9 | §3「当前基线」数值过期（288 用例 / `rust` 11 步） | [✅ 已修] | 第 4 批：旧块改标「**B2 基线（历史数值）**」+ 新增第 4 批基线块（336 用例 / `rust` 16 步 / `DESTDIR` 3+7 文件） |
| D10 | §7 两条守卫描述与 CI 实况不符（core `hux_scheme` 文本守卫实际不存在；平台「只允许 3 路径」实为黑名单） | [✅ 已修] | 第 4 批 `d2f771cb`：**补实现** core `hux_scheme` 文本守卫（剥离注释后匹配），平台黑名单升级为**模块白名单 + 导入名白名单**；文档同步（§7） |
| D11 | §7「注释与文档叙述亦不受影响」与纯文本守卫实况相反 | [✅ 已修] | 第 4 批：新增 `tools/checks/rust_source_grep.py`（剥离注释、保留字符串），三条源码守卫改用它 ⇒ 该句由假变真；负向对照见批次记录 |
| D12 | `README.md` 的 `crates/*/README.md` 索引与实际不符（4 个 crate 无 README） | [✅ 已修] | 第 4 批：改为 `crates/hux-scheme/*/README.md` |
| D13 | §9 与 4 份骨架 README 实况不符（无「依赖方向」/「数据与 API 需求」；`yuhao` 的「学习规则串」契约过期） | [✅ 已修] | 第 4 批：4 份 README 补「依赖方向」+「契约需求」（`Scheme` 回调 / 4 个运行时角色 / `learning_mode` 自算），`wubi`/`shuangpin`/`quanpin` 补「数据需求」 |
| D14 | `docs/usage.md` 手工卸载与 `uninstall.sh` 契约不一致（会连自取模型一起删） | [✅ 已修] | 第 4 批：补「`rm -rf /usr/share/fcitx5/hux` 会连自取模型一起删」+ 按 `data/MANIFEST` 只删随包数据的写法 |
| M1 | `goldens/README.md` 的「重新生成」块缺 pin 检出步骤，不可照抄 | [✅ 已修] | 第 4 批：命令块首加 `set -euo pipefail` + `git -C "$REF" checkout --detach abad411…` + `rev-parse` 自检，并写明只读检出的替代做法（可写克隆 + `fetch <sha>`，**不要 `--depth 1`**）；探针段注明生成器自建隔离工作区、无需 checkout |
| M2 | 「金样不得重生成」的守护不完整：三份 `.gz` 内部 pin/sha 头无人核对；README sha 表无校验器 | [✅ 已修] | 第 4 批：① `key.tsv.gz` 头部**只增不改**补齐（解压后 5132 条记录逐字节不变）；② 新增 `tools/checks/verify_golden_shas.py`（README 表逐行 ↔ 文件、头部 ↔ 声明、`--reference` 按 pin 核参照）接入 `rust` + `golden` 两作业；③ `goldens/local/` 与真实模型抽样口径按审计「可接受」只文档约定 |
| M3 | 探针脚本把**入库夹具**当副作用重写且无校验（上游一变就静默改动另一 pin 的夹具） | [✅ 已修] | 第 5 批 `5304bd35`：两个生成器（`gen_key_sequence_*` / `gen_sound_to_char_shape_*`）加 `guard_fixture`——先写临时产物，与入库夹具 `cmp` 逐字节比对，不一致即失败并区分「pin 变化 / 夹具漂移」，**入库文件不再被改写**；Tab 生成器同构 |
| M4 | 偏离登记完整，但 §8 措辞易读成「两个常量」、两端登记分散 | [✅ 已修] | 本次：§8 该句改写为「偏离用**单一期望值表** `DEVIATIONS` 表达（一个常量服务两份金样），并断言『期望 ≠ 金样』的步集合 == 『实测 ≠ 金样』的步集合」，并写明**文档侧单一来源是 `goldens/README.md` 的偏离说明**（本节给理由与沿革） |
| M5 | `gen_key_golden.sh` 是唯一缺「原子写 + 非空断言」的生成器；`key_probe.cpp` 不检查输入 ⇒ 可把入库 `key.tsv.gz` 静默覆盖成 32 行 | [✅ 已修] | 第 4 批 `9774b639`：`key_probe.cpp` 两个 `ifstream` 加可读性检查（`return 2`）、空输入非零退出；`gen_key_golden.sh` 写 `$OUT.tmp.$$` → 断言至少 1 条 `name` + 1 条 `parse` → `mv`；负向对照 6 组、入库金样未被改动 |
| M6 | `rime_sequence_probe.cpp` 设置选项不检查返回值（方案改键名即静默失效） | [✅ 已修] | 第 5 批：`set_option_checked` 断言「`tiger_sentence_` 前缀的选项已在已部署方案的 `switches` 里声明」，改键名即显式失败；并注明 librime 1.17 的 `set_option` 返回 `void` 且不校验名 ⇒「设完回读」是恒真检查，故不走回读 |
| M7 | 插件路径检查失败时无任何提示（`set -e` 下静默退出） | [✅ 已修] | 第 5 批：三个探针生成器均显式报「生成失败：缺少 librime-lua 插件：$plugin（可用 `LUA_PLUGIN` 覆盖）」 |
| M8 | 依赖 / 版本未固定的位置（action 移动标签、无 `rust-toolchain.toml`、`archlinux:latest`、`librime-dev` 版本） | [待办] | **③④ 已实施 / 已注明**：`pacman -Sy` → `-Syu`；`archlinux:latest` **有意不钉**（作业目的即「最新 Lua」）；已在 `goldens/README.md` 注明 CI 的 librime 版本可不同、仅做语法检查。**①② 未做**（action 钉 commit sha、新增 `rust-toolchain.toml`）：本轮离线，无法验证 GitHub 侧可用性，擅自钉死有让 CI 无预警变红的实际风险——见 §8「工具 / CI」的 `[待办]` |
| M9 | `gen_key_table.py` 直接写目标（含**源码**路径）、`ValueError` 以 traceback 呈现 | [✅ 已修] | 第 5 批：`write_atomic`（同目录 `mkstemp` + 权限对齐 + `os.replace`，非常规文件退回直写）+ `except ValueError` 干净退出（`gen_key_table: <消息>`，退出码 1） |
| M10 | Lua 生成器与 README 的 `gzip > goldens/…` 之间无失败短路（可能压入不完整 TSV） | [✅ 已修] | 第 4 批：命令块首加 `set -euo pipefail` 并把「生成 → 压缩」串起来（与 M1 同批） |
| M11 | 探针两处弱校验：忽略维护失败、不检测用例重名 | [✅ 已修] | 第 5 批：`start_maintenance` 结果入 `check`（先 `join` 再 `check`，失败不留后台线程）；用例名重复即显式失败（含行号） |
| M12 | `tools/cases/key_cases.txt` 有无害重复行（`+`、`Shift++a`） | [已登记·不修+理由] | 第 5 批：重复行**有意保留**——删行会改动入库 `key.tsv.gz` 的记录数（同一输入两次解析必须一致，金样里各出现两次）；已就地加注释说明，避免后人误读为「覆盖两种解析」 |
| M13 | `install.sh` / `uninstall.sh` 在 CI 中完全无守护 | [✅ 已修] | 第 4 批：`rust` 作业加 `Smoke-check entrypoints`——`bash -n` 两个脚本 + 两者 `--dry-run` + `--help`；本机实测与文档一致 |
| M14 | 守卫缺口与过宽（逐条判定表） | [✅ 已修] | 第 4 批：两处「过宽」（core 平台痕迹 / core 角色名字面量）改**剥离注释后匹配**；平台方案引用由黑名单升级为白名单；其余逐条复核为 ✓（「`-p hux-scheme-tiger` 硬编码」与「`env!` 不拦」按现状可接受 / §1.2 已声明有意）。未发现恒真检查 |
| M15 | `install.sh` 用 glob 安装、`uninstall.sh` 用**枚举**删除 ⇒ 漏删风险 | [✅ 已修] | 第 4 批：两侧 + CMake 共用 `data/MANIFEST`，`check_data_manifest.sh` 守护「清单非空 / 无重复 / 文件存在 / 三处都读清单」+ CI `DESTDIR`；负向对照 5 组 |
| M16 | 文档只写 `/usr/lib/fcitx5/`，脚本已兼容 `lib64` | [✅ 已修] | 第 4 批：`docs/usage.md`、`platform/linux/README.md` 补「或发行版 libdir（如 `/usr/lib64/fcitx5/`）」 |
| M17 | 信息项：其余安装 / 卸载契约已核实一致 | [已登记·不修+理由] | **无需动作**：报告自述已逐项实测相符（CMake 3 文件、`--purge` 覆盖面、帮助行 `sed` 范围、`data/README.md` 溯源、`docs/config.md` 14 + 3 项、24 份文档 0 破链）；本轮只复跑了其中的金样 sha 部分（`verify_golden_shas.py` 61 项通过），未逐项重测 |

> 报告 §3「已删 API 在文档中的残留」（12 个符号 / 20 处命中）已在第 4 批逐处处置：删除记录统一标注
> 「已删 / 历史对照，勿在代码中引用」（D6 行），`KeyEvent::forward()` 改写（D7 行），
> `min_retained_raw_length` 等线上字符串保留为**现行事实**并注明归属层。结论与本次一致：
> **没有一处把已删 API 当作现行 API 使用**。

## 9. 骨架（已落地）

- 方案骨架：`crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/README.md`（+ `crates/hux-scheme/README.md`）；
- 平台骨架：`platform/{windows,macos,ios}/README.md`。

每个骨架 README 写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
**仅 README，不进 workspace**，避免空壳与死代码。
