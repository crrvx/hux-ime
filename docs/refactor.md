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
> 当前基线（B2 追平后实测）：`cargo test --workspace` **288 用例全绿**
> （core 72 / tiger 130 / cfg 16 / platform 65 / support 4 / ffi 1；
> 较 B1 的 281 增加 7 个：core 的 `fusion_keys_match_reference_vectors` /
> `fusion_score_is_pairwise_difference` / `fusion_event_encodes_direction_and_offsets`，
> tiger 的 `fusion_ordering_matches_reference_cases` /
> `fusion_ordering_preserves_direct_order_and_reverse_preference` /
> `processor_tab_confirm_stages_against_live_baseline`，
> platform 的 `host_commit_direct_choice_records_no_learning`），
> `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`reuse lint`（152/152）均干净；
> CI `rust` 作业 11 步本机逐步通过；C++ 侧构建链接 + `DESTDIR` 安装 3 文件 + 导出 **14** 个 `hux_*`。

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
    `key_sequence` 仅新增用例（`punct_menu_equal`/`punct_menu_minus`，钉住下面的遮蔽行为）；
    `sound_to_char_shape` 取 `92a0b54`：`digit-zhong` 由「数字并入拼音」变为**直选提交**，
    `nav-page-equal`/`nav-page-minus`/`nav-page-zho` 由「`=`/`-` 翻页」变为「先上屏组合再落标点」，
    并新增撇号/数字/分号用例（31 例 / 164 步）。
  - **⚠️ 需要用户决策的参照行为变化**：`abad411` 的标点分支使**所有**可打印 ASCII 标点（含 `-`/`=`/`[`/`]`）
    在菜单可见时先确认组合、再交标点表 ⇒ 方案侧 `key_binder` 的 `-/=`、`[/]` 翻页绑定在这条路径上被遮蔽
    （`Page_Up`/`Page_Down` 与 `Tab` 循环不受影响）。这是上游参照的实测行为（探针金样 `punct_menu_*` 与
    `nav-page-*`），本仓如实移植并同步了 `README.md`/`docs/usage.md`/`docs/config.md`/`docs/rust-migration.md`；
    若判定为上游缺陷，应在上游修正后随下一批同步。
  - **门槛**：`cargo test --workspace --locked` **291 用例 0 失败**（与 B4 同数：仅改写 2 个平台用例、
    扩 1 个断言表、金样用例内新增覆盖，无净增删）；fmt/clippy/reuse/CI 分层守卫全绿；
    另复验 C++ addon 的 configure→构建→`DESTDIR` 安装布局与 **14 个** `hux_*` 导出符号、
    `gen_pinyin_index.py --check`。

**平台（fcitx5）**
- ✅ 已修：C++ 壳不再硬编码方案选项名——ABI 新增 `hux_engine_option_role_count` /
  `hux_engine_option_key(role)`（角色序 `HUX_OPTION_*`），状态菜单按角色取名、只保留 UI 文案；
  面板数字序号改取**运行时生效值**（`hux_engine_option_value` + `HUX_OPTION_DIGIT_SELECT`），
  状态菜单关掉数字直选后不再显示序号。导出 12 → 14，头文件与 `.so` 符号校验同步通过。
- ✅ 已修：17 项默认值在 C++ schema 与 `hux-cfg::Settings::default` 各写一份的问题，
  由新增测试 `schema_defaults_match_settings_defaults`（解析 `shell/hux.cpp` 的
  `.path{}`/`.defaultValue`，含 keysym→rime 键名转换）逐项比对并断言核对数为 17，
  使漂移在 CI 即失败。
- 面板数字序号/页大小只读配置，而引擎按运行时选项处理（状态菜单关掉数字直选后面板仍显示序号）。
- ✅ 已修：无会话时状态菜单切换直写存储落盘（`OptionsStore::set_value` + 单测）。
- ✅ 已修：`CString` 含 NUL 时剔除并记日志（`ui::cstring_lossy` + 单测），不再整条丢空；
  事件泵上限具名为 `EVENT_PUMP_ROUNDS`。
- ✅ 已修：保存失败属性 `*_options_error` 并入状态串（`hux_engine_status` 可见）+ 端到端单测。
- ✅ 已修（本轮顺带发现的真缺陷）：参照的选项抑制是**写入时**抑制（`live.syncing`），
  本移植曾用「按名一次性名单」——`sync` 排队的事件会**吞掉紧随其后的第一次真实改动**
  （状态菜单在会话建立后首次切换可能不落盘）。现改为写入后 `Context::discard_option_events`
  丢弃自身事件，`Options::observe` 回归参照语义；cfg 单测同步重写。

**① 契约去虎码语义（未开工；方案见下，属跨 4 crate 的大改，建议单独一个会话/批次做完）**
- 病：`OptionIds` 的 4 个字段名与 `SchemeConfig` 的 `min_retained_raw_length`/反查键/Tab 学习
  仍是虎码口径，与 §5「不把方案特有语义泛化进契约」有张力（换方案须改 core）。
- 方案（数据化声明，保持强类型检查在调用侧）：
  1. core：`pub struct OptionDecl { pub role: &'static str, pub key: &'static str }` +
     `Scheme::option_declarations(&self) -> &'static [OptionDecl]`；删 `OptionIds` 与
     `Scheme::option_ids`、`Scheme::learning_mode` 等固定字段入口（`SchemeConfig` 改为
     `&[(role, Value)]` 键值袋，`Value` 为 bool/usize/String 的小枚举）；
  2. `hux-cfg`：不改依赖方向，角色常量归本层（它本就拥有设置词汇），
     `Settings::{option_defaults,store_defaults,option_default,learning_mode}` 改为按
     **角色→键**的已解析表工作（入参为 `&[OptionDecl]` 或由平台解析好的 `HashMap<role,key>`）；
  3. `tiger`：自报 `option_declarations()`（键=其 `interaction::OPTION_*`，角色=cfg 的常量字符串）
     与 `SchemeConfig` 的键值袋解析；虎码语义（早提交/缓冲/锁）留在本 crate；
  4. 平台：装配处把 `scheme.option_declarations()` 交给 cfg 解析，角色缺失即报错
     （原一致性测试升级为「每个角色都必须被方案声明」）。
- 守护：全程 `cargo test --workspace` + 金样；该改动**不动可观测行为**（键名与默认值不变），
  故差分金样应保持逐位一致；`docs/refactor.md` §5 落地清单与 §7 守卫同步。

**内核（hux-core）**
- **契约语义边界**（需决策）：`OptionIds` 的 4 个字段名 + `SchemeConfig` 的 `min_retained_raw_length`/
  反查键/Tab 学习等仍是虎码口径，与 §5「不把方案特有语义泛化进契约」有张力；改为通用容器
  （方案自报键值表）则平台与 cfg 需再改一轮。
- `host.rs` 的 `paging` 条件未实现（`mark_paging` 写的标签全仓无人读；参照 `kWhenPaging`）：
  有候选未翻页时按 `-` 被吞键，参照会落标点。
- `editor` 未实现参照的 `FallbackOptions::All` 与 `Ctrl+Return`/`Ctrl+Shift+Return`（Shift+BackSpace、
  Shift+space、Shift+Delete、Ctrl+Return 参照会消费，core 落 Forward）。
- ✅ 已修：`learning::reward`/`RewardNode` 与方案 `decode::learning_reward` 的重复实现已合并——
  算法只在 core 维护一份，方案把 arena 路径物化为 `RewardNode` 链后调用 core；
  同时删除 `RewardNode.text`（从未被读取的死字段），金样（learning 与 decode+learning）判定顺序正确。
- `session::live_caret` 与 `live_input` 判据不一致 —— ✅ 已修（判据统一为「确实带 `~` 标记」）；
  方案侧是否保留同款副本仍待定（core 外无调用者）。
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
- ✅ 覆盖缺口已补：宿主链绑定（金样走不到——真机路径上 `Return`/`space`/`Escape` 等先在方案
  `processor` 被消费）现有单测钉住：Confirm / Cancel / `Ctrl+BackSpace` / `Ctrl+Return` /
  `Ctrl+Shift+Return` / Shift 回退；`CancelComposition` 按参照 `ClearPreviousSegment() || Clear()` 断言。
- ✅ 已修：翻页键条件按参照（`key_binder.cc:262`）严格化——下翻页 `when: has_menu`、
  上翻页 `when: paging`（`mark_paging` 于翻页时写入标签）；未翻页时上翻页键**不消费**，
  交后续处理器落作标点（参照行为）。core 与平台两侧的单测按新契约重写，
  `docs/config.md`、`docs/rust-migration.md` 的翻页键描述同步。

**工具 / CI**
- ✅ 已修：两个探针生成器写库前先写 `$OUT.tmp.$$` 并断言至少 1 个 `case`，
  再原子 `mv`（空/全注释 CASES 不再把入库金样覆盖成只剩头部）。
- ✅ 已修：`gen_pinyin_index.py --check` 与**由 `--source` 重建**的内容逐字节比对
  （此前不带 `--manifest` 时对任意文件都打印 `check ok`，属恒真检查）；显式传
  `--manifest` 而文件缺失即 `exit 1`。正负例均已验证。
- ✅ 已修：CI 加 `--locked`、`timeout-minutes: 30`（五个作业）与 `concurrency`（同分支取消旧运行）。
  **有意不加** 缓存 action（保持第三方依赖面最小，冷编译代价可接受）。
- ✅ 已修：`addon` 作业加装 `librime-dev` 并对 `tools/probes/*.cpp` 做 **`g++ -fsyntax-only`**
  语法检查（不链接、不运行），防止探针长期无人编译而静默腐坏。本机已实测两个探针语法通过。

## 9. 骨架（已落地）

- 方案骨架：`crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/README.md`（+ `crates/hux-scheme/README.md`）；
- 平台骨架：`platform/{windows,macos,ios}/README.md`。

每个骨架 README 写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；
**仅 README，不进 workspace**，避免空壳与死代码。
