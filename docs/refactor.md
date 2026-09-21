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
> 当前基线：`cargo test --workspace` 267 用例全绿（core 62 / tiger 125 / cfg 16 / platform 59 / support 4 / ffi 1），
> `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`reuse lint` 均干净。

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
> - **P4 收尾 ✅**：选项 id 纳入契约（`Scheme::option_ids` → `OptionIds`），
>   `hux-cfg` 的 4 个硬编码常量与平台的一致性测试一并移除，实现单一来源。

## 5. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案无论如何要依赖 core 的类型（`KeyEvent` / `Context` / `Candidate`…），
  单开 interface crate 只多一跳、无净收益；将来若接口变大或需对外提供「方案作者 SDK」，
  再拆 crate（纯移动 + `pub use` 兜底）。
- **最小契约**：只定义内核必须回调的动作（`id` / 选项声明 / 按键与翻译重建 / 学习策略 / 反查展示；
  落地清单见下文「落地形态」）；
  **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约——先留在 `tiger` profile，
  等第二个同族方案落地后再抽象。
- 形码族（虎码 / 宇浩 / 五笔）优先；拼音族（双拼 / 全拼）只留接口。
- **选项 id 单一来源（✅ 已收口）**：方案经 [`Scheme::option_ids`] 声明自己的选项键
  （`OptionIds`），配置层 `hux-cfg` 不再硬编码方案选项名——`Settings::option_defaults` /
  `store_defaults` / `option_default` 与 `OptionsStore::load`、`options::option_defaults`
  均接收由平台从方案取得的 `&OptionIds`；平台的状态菜单白名单亦据此构造。
  键的**持久化兼容**由方案侧测试 `option_ids_are_stable_persisted_keys` 钉住，
  YAML 读写格式由 `hux-cfg` 的 store 测试（含历史键字面量）守护。
- **内核不 import 任何 `hux-scheme/*`**（校验方式见 §7）。
- **落地形态（P4c）**：`hux_core::scheme::Scheme` 只含「必须回调方案」的动作——
  `id` / `learning_rules` / `option_ids` / `learning_mode` / `apply_config` / `host_options` /
  `set_learning_mode` / `set_store_ready` / `apply_learning_index` / `new_session` / `free_session` /
  `reset_session` / `process_key` / `select_candidate` / `rebuild` / `take_learning_events` /
  `buffered_text` / `auxiliary_lookup_active` / `auxiliary_rows`；
  虎码特有语义（缓冲态、锁、早提交启发式、证据）全部留在 `TigerScheme` 内部。

## 6. 测试与性能

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle。
  现状（P5 后）：源内单测 21 个文件（内核 6 / 方案 8 / 配置 3 / 助手 1 / 平台 3），
  集成与差分 7 个在 `tests/`（内核 2 / 方案 5）——两者不混放。
- `hux-test-support`（P5，**✅ 已落地**，`crates/hux-test-support`）：只放与业务无关的共性工具——
  金样 / 夹具路径定位（`repo_path` / `open_golden`）、transcript 编解码、临时目录（`temp_dir`）；
  各 crate 以 `dev-dependencies` 引入，本 crate 不依赖任何 hux crate（避免测试期成环）。
  **方案专属夹具留在各自 `tests/`**（如 `decode_differential.rs` 的 `make_decoder`）；
  若本 crate 超过约 200 行或开始承载业务逻辑，立即停手、退回各 crate 内 `#[cfg(test)]` 助手。
- `hux-bench` **不新建**：基准以 `--release` 示例提供（`crates/hux-scheme/tiger/examples/{decode_bench,key_bench}.rs`），
  避免新依赖（保持离线可构建）；基线与结论见 [`perf.md`](perf.md)。
- 优化只允许「金样不变」的改动，且须有前后对比数据。
- CI 现状：`rust` 作业（fmt / clippy / **分层测试**：内核+助手 → 方案 → 配置+平台 /
  core 平台痕迹与「core 无方案引用 / 平台不引用方案内部」校验 / 数据溯源）
  + `addon` 作业（cmake configure 与构建链接、`hux_abi.h` ↔ `libhux.so` 符号一致、
  `DESTDIR` 安装布局）+ 金样重生成比对（「层依赖」一步覆盖 §1 规则 1 的四条边）；
  **待补**：`cargo-deny`（可选）。

## 7. 依赖校验

- ✅ 已入 CI（`.github/workflows/ci.yml` 的 Core platform-clean）：`crates/hux-core` 源码不得出现
  环境变量读取 / `SystemTime` / `/usr/share` / `eprintln!` / `println!` 等平台痕迹；
- ✅ 已入 CI：`crates/hux-core` 不得出现 `hux_scheme` / `hux-scheme` 引用，也不得引用已迁出的方案模块
  （`decode` / `lexicon` / `lexical` / `ngram` / `interaction` / 反查）；
- ✅ 已入 CI：`cargo tree` 校验 `hux-core` 无 `hux-scheme/*` 依赖边，且 `hux-scheme/*` 只依赖 `hux-core`；
- ✅ 已入 CI：`platform/fcitx5/src` 只允许 `hux_scheme_tiger::scheme::{TigerScheme, ASSETS, SCHEME_ID}`
  （装配根构造方案），不得引用方案内部模块（`interaction` / `decode` / `lexicon` / …）——即「平台经契约驱动」；
- ✅ 已入 CI：`cargo tree` 校验 `hux-cfg` / `hux-ffi` 不依赖方案（`hux-scheme/*`）与平台层（`hux-platform*`）——§1 规则 1 的四条边全部有守卫；
- 后续可选 `cargo-deny`。

## 8. 复核遗留（P0–P6 全仓复核后登记，按需排期）

> 2026-09-21 全仓复核（5 路并行审计 + 人工核实）已修项见提交 `chore(review)` 三批与
> `fix(review)`；下列为**已核实、尚未实施**的项，连同证据一并登记，避免遗失。

**方案（tiger）追平上游**
- 证据层仍为单权重（等价上游 `12d2ecc`）：参照自 `5ce1ca2` 拆 `base_weight`/`early_weight`
  并引入个性化 `early_commit_confidence_score`，`strong_count` 改看 `base_share`。
- 保留量边界仍用裸 raw 长度：参照自 `d30867a` 改用 `competing_boundary_end`（本 crate 零命中），
  竞争路径更远时本实现会比参照更早提前上屏。
- 两者均**晚于本轮金样 pin（`8b615235`）**，故差分全绿守护不到；追平需升 pin + 重生成金样。

**平台（fcitx5）**
- C++ 壳状态菜单白名单写死 5 个方案选项名（`shell/hux.cpp`），方案改名即静默失效；
  建议经 ABI 暴露运行时选项名，或加一致性测试。
- 17 项默认值在 C++ schema 与 `hux-cfg::Settings::default` 各写一份，无一致性校验。
- 面板数字序号/页大小只读配置，而引擎按运行时选项处理（状态菜单关掉数字直选后面板仍显示序号）。
- 无会话时 `set_option_value` 直接返回 true 而不落盘（状态菜单切换静默丢失）。
- 事件泵硬编码 `0..4`；`CString::new` 失败被静默吞掉（含 NUL 的提交/候选整条丢弃）。
- 保存失败属性 `*_options_error` 全仓无读取方（诊断不落地）。

**内核（hux-core）**
- **契约语义边界**（需决策）：`OptionIds` 的 4 个字段名 + `SchemeConfig` 的 `min_retained_raw_length`/
  反查键/Tab 学习等仍是虎码口径，与 §5「不把方案特有语义泛化进契约」有张力；改为通用容器
  （方案自报键值表）则平台与 cfg 需再改一轮。
- `host.rs` 的 `paging` 条件未实现（`mark_paging` 写的标签全仓无人读；参照 `kWhenPaging`）：
  有候选未翻页时按 `-` 被吞键，参照会落标点。
- `editor` 未实现参照的 `FallbackOptions::All` 与 `Ctrl+Return`/`Ctrl+Shift+Return`（Shift+BackSpace、
  Shift+space、Shift+Delete、Ctrl+Return 参照会消费，core 落 Forward）。
- `learning::reward`/`RewardNode` 无运行时调用者且与方案 `decode::learning_reward` 逐行重复。
- `session::live_caret` 与 `live_input` 判据不一致（前者只看缓冲标志，后者还要求 `~`），
  方案侧有同款副本；core 外无调用者。
- `punct::pair_oddness` 是**可变状态**却放在只读数据表里，`TigerScheme` 单实例 → 多输入上下文串台
  （参照的 `oddness_` 随会话）。
- `reopen_previous_selection` 漏参照的两条护栏（`status > kSelected`、`selected_before_editing`）。
- 纪律对齐：core 内 `std::fs` 读文件（`PunctTable::load_first`）与 §1.2「core 零平台文件 API」
  措辞冲突，CI 正则也只查 env/时钟/打印——需明确「读取平台传入的显式路径」是否在纪律内。
- 清理：`K_HYPER_MASK`/`K_META_MASK`/`KeyEvent::caps`/`Context::has_events` 无使用者；
  `HostResult` 与 `KeyOutcome` 同构重复；`build_valid` 仅是 `event_valid` 的同义包装。

- `editor` 绑定：参照 `ExpressEditor` 表（`_tmp/librime/editor.cc` @ pin `33e78140`）为
  `{Return,0}→CommitRawInput`（✅ 已实现）、`{Return,Ctrl}→CommitScriptText`、
  `{Return,Ctrl+Shift}→CommitComment`、`{BackSpace,0}→RevertLastEdit`、`{BackSpace,Ctrl}→BackToPreviousSyllable`、
  `{Delete,0}→DeleteChar`、`{Delete,Ctrl}→DeleteCandidate`、`{Escape,0}→CancelComposition`，
  另有 `FallbackOptions::All` 的 Shift 回退（✅ 已补 Shift+space / Shift+BackSpace / Shift+Delete）。
  ✅ 已补：`Ctrl+Return` → `CommitScriptText`（按「脚本文本 = 组合文本」实现）、
  `Ctrl+Shift+Return` → `CommitComment`（提交高亮候选注释）。
  **踩坑记录**：模式里的 `K_A | K_B` 是**或模式**而非按位或，组合修饰键必须写成
  `(code, modifier) if modifier == K_A | K_B`；本轮该 bug 已修，并全仓 grep 确认无同类写法。
  **待复核**：`Context::GetScriptText` 的准确定义（应从参照 `context.cc` 核对，本轮网络取源不稳）。
- `paging` 条件：参照 `KeyBinder::RelevantConditions`（`_tmp/librime/key_binder.cc:262`）为
  "末段带 `paging` 标签"；本实现有意放宽为 `has_menu`（`host.rs` `key_binder` 注释），
  字段文档已对齐。若要严格实现，需同时钉住 `paging` 标签的写入时机（`mark_paging` 现由翻页时写入）。

**工具 / CI（廉价加固）**
- 三个不重生成的金样（`key` / `key_sequence` / `sound_to_char_shape`）现已在 CI 校验 sha256 ✓；
  仍缺：`tools/probes/*.cpp` 编译检查、生成器脚本写库前「至少 1 个用例」断言、
  `gen_pinyin_index.py --check` 语义（现恒真）、CI 缓存/超时/`--locked`。

## 9. 骨架（已落地）

- 方案骨架：`crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/README.md`（+ `crates/hux-scheme/README.md`）；
- 平台骨架：`platform/{windows,macos,ios}/README.md`。

每个骨架 README 写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
**仅 README，不进 workspace**，避免空壳与死代码。
