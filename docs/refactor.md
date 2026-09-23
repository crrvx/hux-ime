<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 重构：核心引擎化 / 平台无关 / 码表无关 / 测试正式化

目标：把现状（虎句 + fcitx5 桌面）整理为可承载**多方案、多平台**的引擎结构。 \
**当前范围**：双端（linux / android）+ 虎码（字 / 词 / 句）；其他方案与平台仅留 README 骨架。

## 本文边界

> 章节号保留原编号（§3 / §4 / §8 已移出，故不连续）——代码注释 / CI / 文档里既有的
> 「`refactor.md` §1 / §2 / §5 / §6 / §7 / §9」引用**仍然有效**，无需改号。

- **活规则（本文，只写现状与做法）**：§1 结构正义（硬规则）、§2 目标结构、§5 方案契约、
  §6 测试与性能、§7 依赖校验、§9 骨架。
- **历史与逐批记录** → [`review-ledger.md`](review-ledger.md)：未闭合项（活口，置顶）、迁移映射
  （原 §3，留档）、各轮批次（原 §4）、上游追平、逐批整改记录与四份审计总账。
- **有意偏离上游** → [`upstream-deviations.md`](upstream-deviations.md)：① 翻页 / 标点遮蔽修复
  （含用户决定 B）、② addon 扩展、③ pin 差异、④ 宿主链交互；含金样「字节不动 + `DEVIATIONS`
  可证伪期望值表」策略与回归做法。
- **文档纪律**：活文档只写现状与做法；历史与逐批记录进 `review-ledger.md`，有意偏离进
  `upstream-deviations.md`（见 `AGENTS.md`「背景与约定」）——`refactor.md` 曾因混入流水账达 1100+ 行。

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
                              #   （options.yaml > 设置 > 内建；配置页推送时设置值写回存储）、
                              #   状态菜单开关白名单、持久化接口
  hux-ffi/                    # C ABI：C 布局类型 + 导出函数（桌面 / Android 共用）
  hux-scheme/
    tiger/                    # 虎码（字/词/句）——当前唯一全量实现
    yuhao/  wubi/             # init：README 骨架（形码族，复用 tiger 框架）
    shuangpin/  quanpin/      # init：README 骨架（拼音族，接口预留）
  hux-test-support/           # 测试助手（金样路径 / transcript 编解码 / 临时目录）
platform/                     # 平台适配
  fcitx5/                     # 共享 fcitx5 适配：Rust 组装（Engine/UI 快照/存储实现/Paths）
                              #   + C++ 壳 + CMake（linux 与 android 共用）
  linux/                      # 桌面：构建 / 安装说明（入口脚本在仓库根；打包待做）
  android/                    # Android：构建接线（对接 fcitx5-android fork 的 plugin/hux）
  windows/  macos/  ios/      # init：README 骨架
```

平台层分工：`platform/fcitx5` 是**共用适配**（两端都是 fcitx5，环境变量与路径解析同一套）；
`platform/linux`、`platform/android` 只管各自的**构建与分发**。

主要模块（文件级）：

- `hux-core`：`cache` / `learning` / `key` / `key_table` / `session` / `host` / `punct` / `scheme`（方案契约）；
- `hux-scheme/tiger`：`lexicon` / `decode` / `lexical` / `ngram` / `sound_to_char_shape` /
  `char_to_sound_shape` / `interaction`（+ `interaction/`）；
- `hux-cfg`（设置与默认值、选项存储与合并顺序）、`hux-ffi`（C 布局类型 + `include/hux_abi.h`，
  桌面 / Android 共用）、`hux-test-support`（dev 依赖：金样路径 / transcript 编解码 / 临时目录）；
- `platform/fcitx5`：C++ 薄壳（`shell/hux.cpp`）+ Rust 组装（`engine` / `session` / `ui` / `paths` /
  `learning_store` / `abi`，导出 C ABI）。

其余目录：`data/`（随包数据源）、`assets/branding/`（多平台共享品牌图形，唯一源是 SVG）、
`goldens/`（差分金样与夹具）、
`tools/`（金样生成器 `generators/`、探针与基准 `probes/`、探针用例 `cases/`）、
`docs/`（设计 / 重构 / 使用 / 配置 / 性能 / Android 等，索引见根 `README.md`「文档」表）；
`platform/android` 的插件接线**待启动**，见 [`android.md`](android.md)；`platform/linux` 的打包待做。

参照实现 → Rust 的模块映射（含各模块差分手段）见 [`design.md`](design.md) §2。

> **现状**：`crates/hux-cfg`、`crates/hux-ffi`、`crates/hux-scheme/tiger`、`platform/fcitx5`、
> `platform/linux` 均已落地；`hux-core` 只余通用内核（cache/collections/key/key_table/learning/punct/session/host）
> **+ 方案契约 `hux_core::scheme`**；平台装配根构造 tiger 后以 `dyn Scheme` 驱动。

## 5. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案无论如何要依赖 core 的类型（`KeyEvent` / `Context` / `Candidate`…），
  单开 interface crate 只多一跳、无净收益；将来若接口变大或需对外提供「方案作者 SDK」，
  再拆 crate（纯移动 + `pub use` 兜底）。
- **最小契约**：只定义内核必须回调的动作（`id` / 选项声明 / 按键与翻译重建 / 学习策略 / 反查展示；
  落地清单见下文「落地形态」）；
  **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约——先留在 `tiger` profile，
  等第二个同族方案落地后再抽象。
- 形码族（虎码 / 宇浩 / 五笔）优先；拼音族（双拼 / 全拼）只留接口。
- **选项键单一来源 + 角色归配置层（记录见 [`review-ledger.md`](review-ledger.md) §4.2）**：方案经 `Scheme::option_declarations`
  自报「角色 → 键」声明（`&'static [OptionDecl]`）；**角色词汇与默认值归 `hux-cfg`**
  （`hux_cfg::roles` 的常量；宿主标准项 `full_shape` / `ascii_punct` 由配置层自持，不由方案声明）。
  平台在装配处把声明解析为 `OptionKeys` 角色表（**缺角色即报错**，不静默接线），
  `Settings::{option_defaults, store_defaults, option_default}` 与 `options::option_defaults`、
  `OptionsStore::load` 均按该表工作；平台的状态菜单白名单亦据此构造（角色序 = C ABI `HUX_OPTION_*` 序）。
  键的**持久化兼容**由方案侧测试 `option_declarations_are_stable_persisted_keys` 钉住，
  「每个角色都必须被方案声明」由平台测试 `every_configured_role_is_declared_by_the_scheme` 钉住，
  YAML 读写格式由 `hux-cfg` 的 store 测试（含历史键字面量）守护。
  **角色一致性守护（记录见 [`review-ledger.md`](review-ledger.md) §4.2）**：`hux-cfg` 与方案各自持有一份同值字面量，
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
- **口径命名**：配置 / ABI / 平台层的**标识符**描述引擎概念
  （`ROLE_MIN_RETAINED_INPUT_LENGTH`、`ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS`、
  `ROLE_REVERSE_LOOKUP_CHARACTER_KEYS`、`ROLE_LEARNING_ON_TAB` 及对应的 `Settings` 字段 /
  `hux_options` 成员 / C++ 配置成员）；**线上字符串一律不动**——`ROLE_*` 的值仍是与上游 schema /
  rime 选项同名的键（`"min_retained_raw_length"` / `"sound_to_char_shape_keys"` /
  `"char_to_sound_shape_keys"` / `"tab_learning"`），`shell/hux.cpp` 的 `.path{}` / schema 默认值路径、
  `tiger_sentence_*` 前缀、学习库目录名、`options.yaml` 与 `Library` / `Icon` 也保持原样。
  方案侧的**模块 / 函数名**（`sound_to_char_shape` / `char_to_sound_shape` 及其内部 helper）
  是参照移植的溯源名，不在此列。
- **方案配置袋（见 [`review-ledger.md`](review-ledger.md) §4.2）**：`SchemeConfig` 是「角色 → `Value`（开关 / 计数 / 文本 / 文本列表）」的
  **通用键值袋**——平台按角色装配（角色全集 `hux_cfg::roles::SCHEME_CONFIG_ROLES`，装配完整性由平台测试
  `scheme_config_covers_every_declared_role` 守护），方案按角色解释；内核不再出现
  `min_retained_raw_length` / 反查键 / Tab 学习等虎码口径字段，换方案不必改 core。
- **内核不 import 任何 `hux-scheme/*`**（校验方式见 §7）。
- **配置诊断通道（记录见 [`review-ledger.md`](review-ledger.md) §4.3）**：`Scheme::apply_config(&SchemeConfig) -> Result<(), Vec<ConfigError>>`
  ——`SchemeConfig::require_{bool,count,text,texts}` 把「角色缺失」与「类型不符」区分开
  （`ConfigError` 带角色名与期望类型），方案按缺省值回退的同时把诊断回给平台；
  平台并入状态串（`config:` 前缀，与装配期「未识别的角色 / 缺少角色」同风格）。
  `texts` 相应改为 `Option<&[String]>`（不再把「类型不符」退化成空切片）。
- **落地形态**：`hux_core::scheme::Scheme` 只含「必须回调方案」的动作——
  `id` / `option_declarations` / `learning_mode` / `apply_config` / `host_options` /
  `set_store_ready` / `apply_learning_index` / `new_session` / `free_session` /
  `reset_session` / `process_key` / `select_candidate` / `rebuild` / `take_learning_events` /
  `buffered_text` / `auxiliary_lookup_active` / `auxiliary_rows`；
  学习 mode 由方案据配置袋**自算**（平台只取不透明串 `Scheme::learning_mode`），
  虎码特有语义（缓冲态、锁、早提交启发式、证据、mode 串格式）全部留在 `TigerScheme` 内部。

## 6. 测试与性能

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle。
  现状：含内联单测的源文件 **25 个**（内核 8 / 方案 8 / 配置 4 / 助手 1 / 平台 3 / ffi 1），
  集成与差分 7 个在 `tests/`（内核 2 / 方案 5）——两者不混放。
  （数法为 `grep -rl '#\[cfg(test)\]' crates platform`。）
- `hux-test-support`（`crates/hux-test-support`）：只放与业务无关的共性工具——
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
  （「层依赖」一步覆盖 §1 规则 1 的四条边）；`rust` 作业为 16 步；
  **待补**：`cargo-deny`（可选）、CI action 钉 commit sha（[`review-ledger.md`](review-ledger.md) §0 的
  `[待办]`）；Rust 工具链**有意跟随最新 stable**（不钉 `rust-toolchain.toml`）。

## 7. 依赖校验

> **统一做法**：源码文本类守卫一律先**剥离 Rust 注释**（`//`、`///`、`/* */`
> 含嵌套，字符串字面量保留）再匹配——工具是 `tools/checks/rust_source_grep.py`
> （`--mode no-comments`）。故「注释里写为什么不能出现
> 某个角色名」不再让 CI 变红，而真代码里的字面量照旧命中；依赖边判定仍由 `cargo tree` 负责。

- ✅ 已入 CI（`.github/workflows/ci.yml` 的 Core platform-clean）：`crates/hux-core` 源码不得出现
  环境变量读取 / `SystemTime` / `/usr/share` / `eprintln!` / `println!` 等平台痕迹（注释除外）；
- ✅ 已入 CI：`crates/hux-core` 不得出现 `hux_scheme` / `hux-scheme` 引用（**注释除外**——
  `lib.rs` / `scheme.rs` / `host.rs` / `session.rs` 里「不依赖 hux-scheme」的说明性提及是合法的，实测 6 处），
  也不得引用已迁出的方案模块（`decode` / `lexicon` / `lexical` / `ngram` / `interaction` / 反查）；
- ✅ 已入 CI：`cargo tree` 校验 `hux-core` 无 `hux-scheme/*` 依赖边，且 `hux-scheme/*` 只依赖 `hux-core`；
- ✅ 已入 CI：`platform/fcitx5/src` 的方案引用走**白名单**（此前只是「内部模块黑名单」）：
  ① 出现的方案模块只能是 `hux_scheme_tiger::scheme`（装配根构造方案，`hux_scheme_tiger::<其它模块>` 一律失败）；
  ② 从 `scheme` 大括号导入的名字只允许 `ASSETS` / `TigerScheme` / `SCHEME_ID`；
  ③ 保留原有内部模块黑名单（`interaction` / `decode` / `lexicon` / …）——即「平台经契约驱动」；
- ✅ 已入 CI：`cargo tree` 校验 `hux-cfg` / `hux-ffi` 不依赖方案（`hux-scheme/*`）与平台层（`hux-platform*`）——§1 规则 1 的四条边全部有守卫；
- ✅ 已入 CI（契约去方案语义时新增；记录见 [`review-ledger.md`](review-ledger.md) §4.2）：`crates/hux-core` 不得出现**带引号的**角色名 / 方案选项键字面量
  （`"tab_learning"`、`"tiger_sentence_<…>"` 等）——角色词汇归 `hux-cfg`、键归方案；
  rime 标准名 `full_shape` / `ascii_punct` 由 core 宿主链自持，不在此列（**注释与文档叙述确实不受影响**：
  守卫剥离注释后匹配（避免 `grep` 误伤注释）；
- 后续可选 `cargo-deny`。

## 9. 骨架（已落地）

- 方案骨架：`crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/README.md`（+ `crates/hux-scheme/README.md`）；
- 平台骨架：`platform/{windows,macos,ios}/README.md`。

每个骨架 README 写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
**仅 README，不进 workspace**，避免空壳与死代码。
