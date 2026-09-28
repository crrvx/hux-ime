<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 设计

- **hux-ime（虎虚）**：虎句（`tiger_sentence`）的 fcitx5 原生实现，全 Rust
- 参照实现 [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)：仅作测试 oracle（开发 / CI）
- 本文是**活规则 + 设计现状**的单一来源
- 查表式参考（模块映射、数据与目录）见 [`reference.md`](reference.md)
- 金样清单与格式见 [`../goldens/README.md`](../goldens/README.md)
- 金样再生见 [`../goldens/REGENERATE.md`](../goldens/REGENERATE.md)，校验和见 \
  [`../goldens/PROVENANCE.md`](../goldens/PROVENANCE.md)
- 历史与逐批记录见 [`review-ledger.md`](review-ledger.md)；有意偏离（编号沿用原 §8）见 \
  [`upstream-deviations.md`](upstream-deviations.md)；文档分工与纪律见 `AGENTS.md`

## 1. 结构与硬规则

1. **依赖单向**：`platform/*` 只依赖 `hux-ffi` / `hux-cfg` / `hux-core`；`hux-cfg → hux-core`、\
   `hux-scheme/* → hux-core`；内核不依赖任何方案与平台
   - 对具体方案（`platform/* → hux-scheme/*`）只在**装配处**依赖，且只经契约与装配面常量，\
     不得引用方案内部模块
2. **core 零平台**：不得出现 `std::env`、XDG / 绝对数据路径解析、`SystemTime::now`、`eprintln!`；\
   路径 / 时钟 / 日志由平台构造并注入（目录列表、`now: f64`、notes 汇总）
   - 允许按平台传入的**显式路径**读取，如 `PunctTable::load` 按给定路径读 `symbols.yaml`——\
     core 只做「给定路径 → 解析」，不解析环境、不拼接平台目录
   - CI 的 platform-clean 据此只拦 env / XDG / 时钟 / 直接打印
3. **职责归位**：只读数据（码表 / 模型 / 索引）属方案；可写数据（选项 / 学习库）属 `hux-cfg` 与\
   平台存储实现；UI 快照、C++ 壳、打包属 `platform`；C ABI 是平台边界（`hux-ffi`）
4. 模块名即职责；单元测试随模块，集成 / 差分测试独立目录（二者分离）
5. **确定性**：凡排序必带全序 tie-breaker；凡 `pairs` 影响可观测结果处显式排序；时间 / 随机全部注入

**目标结构**：

```text
crates/                       # 平台无关的 Rust 库
  hux-core/                   # 内核：key / session / composition / 处理器管线 / 宿主链 / 学习机制
                              #   + 方案契约（hux_core::scheme）
  hux-cfg/                    # 配置层：设置项与默认值、选项存储与合并顺序、状态菜单开关白名单
  hux-ffi/                    # C ABI：C 布局类型 + 导出函数（桌面 / Android 共用）
  hux-scheme/
    tiger/                    # 虎码（字 / 词 / 句）——当前唯一全量实现
    yuhao/  wubi/             # 计划：形码族骨架（复用 tiger 框架）
    shuangpin/  quanpin/      # 计划：拼音族骨架（接口预留）
  hux-test-support/           # 测试助手（金样路径 / transcript 编解码 / 临时目录）
platform/                     # 平台适配
  fcitx5/                     # 共用适配：Rust 组装（Engine / UI 快照 / 存储 / 目录拼接）+ C++ 壳
  linux/                      # Linux 落点：XDG 目录根规则、打开目录、安装规则（CMake）、脚本
  android/                    # Android 落点：宿主注入的目录根规则、打开目录占位、fork 构建接线
  windows/  macos/  ios/      # 计划：未建目录（平台总览见 platform/README.md）
```

- 两个落点都基于 `platform/fcitx5`：同一份 addon，差异只在目录根规则、打开目录与装配变量
- 平台层分工：共用适配 = `fcitx5/`，落点 = `linux/` 与 `android/`；文件级模块与参照映射见 \
  [`reference.md`](reference.md) §1
- **现状**：`crates/hux-cfg`、`crates/hux-ffi`、`crates/hux-scheme/tiger`、`platform/fcitx5`、\
  `platform/linux` 均已落地；`hux-core` 只余通用内核（cache / collections / key / key_table / \
  learning / punct / session / host）加方案契约 `hux_core::scheme`；平台装配根构造 tiger 后以 \
  `dyn Scheme` 驱动（Rust 组装含 `engine` / `session` / `ui` / `paths` / `learning_store` / `abi`）
- 其余目录：`data/`（随包数据源）、`assets/branding/`（品牌图形，主源 `hux.png`）、\
  `docs/`（索引见根 `README.md`「文档」表）
- `platform/android` 插件接线**待启动**，PKGBUILD 打包待做 \
  （见 [`../platform/README.md`](../platform/README.md)）

## 2. 方案契约（`hux_core::scheme`）

- **放 `hux-core`**：方案依赖 core 类型（`KeyEvent` / `Context` / `Candidate`…），单开 interface \
  crate 无净收益；将来接口变大或需「方案作者 SDK」再拆（拆法：纯移动 + `pub use` 兜底）
- **最小契约**：只定义内核必须回调的动作——`id`、选项声明、按键与翻译重建、学习策略、反查展示；\
  - **不把虎码特有语义**（缓冲态、锁、早提交启发式）泛化进契约：先留 `tiger` profile
  - 等第二个同族方案落地再抽象：形码族（虎码 / 宇浩 / 五笔）优先，拼音族（双拼 / 全拼）只留接口
- **选项键单一来源 + 角色归配置层**：
  - 方案经 `Scheme::option_declarations` 自报「角色 → 键」，声明类型 `&'static [OptionDecl]`
  - 角色词汇与默认值归 `hux-cfg`（`hux_cfg::roles`）；宿主标准项 `full_shape` / `ascii_punct` \
    由配置层自持
  - 平台装配处解析为角色表 `OptionKeys`；该处**缺角色即报错**，不静默接线
  - 按该表工作：`Settings::{session_option_defaults, store_defaults, session_option_default}`、\
    `options::builtin_option_defaults`、`OptionsStore::load`、状态菜单白名单（角色序 = C ABI \
    `HUX_OPTION_*` 序）
- **四张清单互相钉住**：
  - 键的持久化兼容 → 方案测试 `option_declarations_are_stable_persisted_keys`；YAML 读写由 \
    `hux-cfg` store 测试（含历史键字面量）守护
  - 「每个角色都必须被方案声明」→ 平台测试 `every_configured_role_is_declared_by_the_scheme`
  - 角色一致性三重守护：① 方案自报 `hux_scheme_tiger::scheme::SCHEME_CONFIG_ROLES`，\
    `TigerScheme::load` 报「未识别 / 缺少角色」诊断并进 `hux_engine_status`；② 平台测试 \
    `scheme_config_roles_match_the_scheme`（真实装配路径无诊断、改名必报诊断）、\
    `runtime_role_tables_cover_the_declared_roles`（`SCHEME_OPTION_ROLES` ⊆ `RUNTIME_OPTION_ROLES`，\
    存储 / 会话缺省覆盖运行时角色）、`option_role_order_matches_the_abi_header`（`hux_abi.h` 的 \
    `HUX_OPTION_*` 枚举序 ↔ `RUNTIME_OPTION_ROLES`）；③ `HUX_OPTION_COUNT` 配 C++ 静态断言 \
    `static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`，把「加角色未补文案」从越界读（UB）\
    变成编译失败
  - 风险来源（故加守护）：两份同值字面量（`hux-cfg` / 方案）加 `Config::parse` 对未知角色 \
    `unwrap_or(0/false)` 的静默回退——单侧改名可让 `min_retained_raw_length` / `high_freq_limit` \
    静默失效，测试仍全绿
- **口径命名**：配置 / ABI / 平台层的**标识符**描述引擎概念（`ROLE_MIN_RETAINED_INPUT_LENGTH`、\
  `ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS`、`ROLE_REVERSE_LOOKUP_CHARACTER_KEYS`、\
  `ROLE_LEARNING_ON_TAB`，及对应 `Settings` 字段 / `hux_options` 成员 / C++ 配置成员）
  - **线上字符串一律不动**：`ROLE_*` 的值仍与上游 schema / rime 同名（`"min_retained_raw_length"`、\
    `"sound_to_char_shape_keys"`、`"char_to_sound_shape_keys"`、`"tab_learning"`），另有 \
    `shell/hux.cpp` 的 `.path{}` 与 schema 默认值路径、`tiger_sentence_*` 前缀、学习库目录名、\
    `options.yaml`、`Library` / `Icon`
  - 方案侧的**模块 / 函数名**（`sound_to_char_shape` / `char_to_sound_shape` 及内部 helper）\
    是参照移植的溯源名
- **方案配置袋**：`SchemeConfig` 是「角色 → `Value`」的通用键值袋（`Value` 可取开关 / 计数 / 文本 / \
  文本列表）；平台按角色装配（全集 `hux_cfg::roles::SCHEME_CONFIG_ROLES`），完整性由平台测试 \
  `scheme_config_covers_every_declared_role` 守护；方案按角色解释。内核不再有 \
  `min_retained_raw_length` / 反查键 / Tab 学习等虎码口径字段，换方案不必改 core
- **内核不 import 任何 `hux-scheme/*`**（校验见 §4）
- **配置诊断通道**：`Scheme::apply_config(&SchemeConfig) -> Result<(), Vec<ConfigError>>`；入口 \
  `SchemeConfig::require_{bool,count,text,texts}` 区分失败（角色缺失 / 类型不符），`ConfigError` \
  带角色名与期望类型；方案按缺省回退并回诊断，平台并入状态串（`config:` 前缀）；`texts` 改为 \
  `Option<&[String]>`
- **落地形态**：`hux_core::scheme::Scheme` 只含必须回调方案的动作：
  - `id` / `option_declarations` / `learning_mode`
  - `apply_config` / `host_options` / `set_store_ready`
  - `apply_learning_index` / `new_session` / `free_session` / `reset_session`
  - `process_key` / `select_candidate` / `rebuild`
  - `take_learning_events` / `buffered_text`
  - `auxiliary_lookup_active` / `auxiliary_rows`
  - 学习 mode 由方案据配置袋**自算**，平台只取不透明串 `Scheme::learning_mode`
  - 虎码特有语义（缓冲态、锁、早提交启发式、证据、mode 串格式）全留 `TigerScheme`

## 3. 测试与性能纪律

- 单元测试随模块；集成 / 差分测试独立 `tests/`；金样只读，持续作为行为 oracle
- 现状：含内联单测的源文件 **25 个**（内核 8 / 方案 8 / 配置 4 / 助手 1 / 平台 3 / ffi 1）；\
  集成与差分 7 个在 `tests/`（内核 2 / 方案 5）；数法 `grep -rl '#\[cfg(test)\]' crates platform`
- `hux-test-support`（各 crate 以 `dev-dependencies` 引入）：
  - 只放与业务无关的共性工具：金样 / 夹具路径定位（`repo_path` / `open_golden`）、transcript \
    编解码、临时目录（`temp_dir`）
  - 本 crate 不依赖任何 hux crate（避免成环）；**方案专属夹具留在各自 `tests/`**
  - 超过约 200 行或承载业务逻辑即停手，退回各 crate 内 `#[cfg(test)]` 助手
- `hux-bench` **不新建**：基准以 `--release` 示例提供 \
  （`crates/hux-scheme/tiger/examples/{decode_bench,key_bench}.rs`），避免新依赖（离线可构建）
- **优化只允许「金样不变」的改动，且须有前后对比数据**（用法、基线与结论见 §10）
- CI 的 `rust` 作业（共 **16 步**）：fmt / clippy / **分层测试**（内核+助手 → 方案 → 配置+平台）；\
  另含 core 平台痕迹校验、core 无方案引用 / 平台不引用方案内部校验、数据溯源校验、\
  **金样 sha 表与内部头部校验**（`tools/checks/verify_golden_shas.py`）、\
  **装-卸-CMake 清单一致自检**（`tools/checks/check_data_manifest.sh`）、两个一键脚本的 \
  `bash -n` + `--dry-run` 冒烟；「层依赖」一步覆盖 §1 规则 1 的四条边
- `addon` 作业：cmake configure 与构建链接；`hux_abi.h` ↔ `libhux.so` 符号一致；`DESTDIR` \
  安装布局 = 3 个插件文件 + `data/MANIFEST` 全部随包数据；金样重生成比对
- **待补**：CI action 钉 commit sha、可选的 `cargo-deny`（活口见 [`open-items.md`](open-items.md) \
  §1 的 `M8` 补记）
- Rust 工具链**有意跟随最新 stable**（不钉 `rust-toolchain.toml`）

## 4. 依赖校验

- **统一做法**：源码文本类守卫一律先**剥离 Rust 注释**（`//`、`///`、`/* */` 含嵌套）再匹配，\
  字符串字面量保留；工具 `tools/checks/rust_source_grep.py --mode no-comments`
- 好处：注释里写角色名不会让 CI 变红，真代码的字面量照旧命中；依赖边由 `cargo tree` 判定
- 以下均已入 CI：
  - `crates/hux-core` 不得出现环境变量读取与平台痕迹（`SystemTime` / `/usr/share` / \
    `eprintln!` / `println!`，注释除外）——`.github/workflows/ci.yml` 的 Core platform-clean
  - `crates/hux-core` 不得出现 `hux_scheme` / `hux-scheme` 引用（**注释除外**），\
    也不得引用已迁出的方案模块（`decode` / `lexicon` / `lexical` / `ngram` / `interaction` / 反查）
  - `cargo tree`：`hux-core` 无 `hux-scheme/*` 边；`hux-scheme/*` 只依赖 `hux-core`；`hux-cfg` / \
    `hux-ffi` 不依赖方案与平台层（`hux-scheme/*`、`hux-platform*`）——即 §1 规则 1 的四条边全部有守卫
  - `platform/fcitx5/src` 的方案引用走**两级白名单**：① 方案模块只能是 `hux_scheme_tiger::scheme`，\
    其它 `hux_scheme_tiger::<其它模块>` 一律失败；② 从 `scheme` 大括号导入的名字只允许 `ASSETS` / \
    `TigerScheme` / `SCHEME_ID`；③ 另有内部模块黑名单（`interaction` / `decode` / `lexicon` / …）\
    ——即「平台经契约驱动」
  - `crates/hux-core` 不得出现**带引号的**角色名 / 方案选项键字面量（如 `"tab_learning"`、\
    `"tiger_sentence_<…>"`）：角色词汇归 `hux-cfg`、键归方案；rime 标准名 `full_shape` / \
    `ascii_punct` 由 core 宿主链自持，不在此列
- 后续可选 `cargo-deny`

## 5. 骨架（已落地）

- 方案骨架 `crates/hux-scheme/{yuhao,wubi,shuangpin,quanpin}/`，见 \
  [`../crates/hux-scheme/README.md`](../crates/hux-scheme/README.md)
- 平台骨架 `platform/{windows,macos,ios}/`，见 [`../platform/README.md`](../platform/README.md)
- 每个骨架目录写明：目标、与 tiger / fcitx5 的差异、数据与 API 需求、依赖方向；**仅目录与说明，\
  不进 workspace**，避免空壳死代码

## 6. 模块映射（参照 → Rust）

- 映射表见 [`reference.md`](reference.md) §1（单一来源）

## 7. 数据与目录

- 数据与目录解析见 [`reference.md`](reference.md) §2（单一来源）

## 8. fcitx5 集成要点

- **注册与构建**：addon 元数据 + 输入法条目 conf；C++ 薄壳链接 Rust 静态库；构建 / 安装与落点见 \
  [`install.md`](install.md)「安装」
- **会话**：每输入上下文一个（`InputContextProperty`；暂存隔离，选项为引擎级）；失焦 / 切换见 \
  [`platform/README.md`](../platform/README.md)
- **组合重建**由 `interaction::CompositionBuilder` 按参照 `Compose` 语义：`input[..caret]`、\
  公共前缀增量保留、提交后旧段不复用
- **宿主语义**（core `host.rs`，`processor` 返回 Forward 后执行）：

  | 组件 | 行为要点 |
  |---|---|
  | `key_binder` | `Tab`→Down、`Shift+Tab`→Up（`when: has_menu`） |
  | `selector` | 菜单导航与翻页：`page_size`、翻页键与翻页循环可由<br>配置覆盖；默认 `-`/`[` → Page_Up、`=`/`]` → Page_Down<br>**两侧同前置**：菜单可见（且非 `ascii_mode`）即判翻页<br>（用户决定 B，见下）；Home/End；候选排列由配置写入 `_vertical` |
  | `navigator` | 字节光标移动；Ctrl/Shift+Left/Right 跳到段首 / 段尾<br>（未做音节 spans 细分）；Home/End 到组合起点 / 末尾 |
  | `express_editor` | space 确认 / 提交、BackSpace 撤销编辑、Delete 删光标处、<br>Return 提交原文、Escape 取消；可打印字符先提交组合再交宿主 |
  | `punctuator` | 单键可打印 ASCII 查 `symbols.yaml`；组合中提交<br>「组合文本 + 标点」；`{pair}` 交替 |
  - 参照 `Selector::PreviousPage` 在首页也 `Highlight(0)`（`menu/page_down_cycle` 只作用于 \
    `NextPage`）；参照另写的 `paging` 标签随其唯一读取方一并删除
  - **翻页键不被标点分支遮蔽**（本仓有意偏离上游 `abad411`）：判据 \
    `hux_core::host::paging_action(...)` 两侧共用；**代价**是菜单可见时这几个键打不出标点 \
    （依据与最小复现见 [`upstream-deviations.md`](upstream-deviations.md) ①）
- **英文模式不实现**（设计取舍）：英文输入交由 fcitx5 切换输入法；大写字母经 `char_handler` \
  直通（先提交组合）
- **提交与按键顺序**：处置位 `HUX_KEY_FORWARD_AFTER_COMMIT`（在 `crates/hux-ffi/include/hux_abi.h`）\
  在提交后重发按键；语义与例外见 [`platform/README.md`](../platform/README.md)
- **UI 同步**：preedit 参照 librime `Composition::GetPreedit`——高亮候选的 `preedit` 优先（按词分码 \
  如 `sh ks`，反查段按音节），其后原始输入原样接续（移动光标时保持分码，如 `` ab cd `` + 尾部 `ja` \
  → `` ab cdja ``）；无高亮候选回退「缓冲 + 原始输入」，光标为字节偏移
- **反查**：对齐 librime 词典反查；音侧 `sound_to_char_shape.rs`、字侧 `char_to_sound_shape.rs`，\
  契约见 [`platform/README.md`](../platform/README.md)
- **学习**：提交点通知器（参照 `Context::Commit`）覆盖核心路径与宿主链提交点；\
  `LiveLearning::submitted` 排空落库，`store_ready` 后生效；库上限 1 万条 / 16 MiB、60 秒节流；\
  提交点范围见 [`platform/README.md`](../platform/README.md)，存储见 [`config.md`](config.md)
- **选项键与配置**：方案经 `Scheme::option_declarations` 自报「角色 → 键」，角色词汇在 \
  `hux-cfg::roles`，平台装配处解析成角色表、**缺角色即报错**（状态串可见）；配置项与选项存储见 \
  [`config.md`](config.md)（含只读回退 `user.yaml` 的 `var/option/*`、保存失败属性 \
  `tiger_sentence_options_error`）；图形 schema 与平台接线见 \
  [`platform/README.md`](../platform/README.md)

## 9. 测试

- **Rust 差分**：模块对金样逐位断言
- **键序列金样**：真 librime 探针生成「键序列 → 提交 / 候选 / 预编辑」，Rust 重放比对
- **CI**：fmt / clippy / 差分 + 固定参照提交重生成 fixture 金样比对
- 清单与格式见 [`goldens/README.md`](../goldens/README.md)；重生成见 \
  [`goldens/REGENERATE.md`](../goldens/REGENERATE.md)；sha 表见 \
  [`goldens/PROVENANCE.md`](../goldens/PROVENANCE.md)
- 分层、CI 作业与工具链纪律见 §3

## 10. 性能

- 纪律见 §3（**金样不变** + 前后对比数据）；基准是两个 `--release` 示例（另有 `ngram_bench.rs`），\
  不引入 `criterion`，保持离线可构建：

```sh
# decode 冷路径：重放 goldens/decode.tsv.gz 的 847 条输入（与差分测试同一批语料）
cargo run --release --example decode_bench
cargo run --release --example decode_bench -- --model goldens/ngram_fixture.bin \
    --lexical data/tiger_sentence.lexical.bin

# 整键路径：经方案契约驱动会话，测「process_key + rebuild」单键耗时
cargo run --release --example key_bench
cargo run --release --example key_bench -- --model goldens/ngram_fixture.bin
```

- 参数：`decode_bench` 支持 `--model <bin>` / `--lexical <bin>` / `--repeat N`；`key_bench` 支持 \
  `--codes N` / `--repeat N` / `--model <bin>`
- 输出：`decode_bench` 一行 JSON（`corpus` / `repeat` / `ops` / `mean_us` / `p50_us` / `p95_us` / \
  `max_us` / `checksum`），其后另加按输入长度分桶的 5 行文本；`key_bench` 一行 JSON（`codes` / \
  `repeat` / `keys` / `model` / `mean_us` / …，无 `checksum`）。`checksum` 用于确认测量期间计算 \
  真的发生了且结果稳定
- **基线**：2026-09-21，开发机 Arch + release；`decode_bench --repeat 20`、`key_bench --repeat 10`

| 场景 | p50 | p95 | max | 说明 |
| --- | --- | --- | --- | --- |
| decode 冷路径（无模型） | **1.00 µs** | 2.29 µs | 4779 µs | 16,940 次 |
| decode 冷路径（fixture 模型） | 1.06 µs | 2.56 µs | 2605 µs | 16,940 次 |
| decode 冷路径（+ 真实词先验位图） | 1.08 µs | 2.58 µs | 2671 µs | 16,940 次 |
| 整键路径（无模型） | **2.41 µs** | 181 µs | 2670 µs | 4,990 次 |
| 整键路径（fixture 模型） | 2.71 µs | 204 µs | 2871 µs | 4,990 次 |

- 按输入长度分桶（decode 冷路径，无模型）：

| 输入长度 | 样本 | p50 | p95 | max |
| --- | --- | --- | --- | --- |
| 1–2 字符 | 14,080 | 0.96 µs | 1.69 µs | 786 µs |
| 3–5 字符 | 2,820 | 1.63 µs | 3.38 µs | 10.3 µs |
| > 20 字符 | 40 | **2278 µs** | 2355 µs | 4779 µs |

- **结论**：
  - 打字路径已是微秒级：1–5 字符（占语料 99.8%）decode p95 ≤ 3.4 µs，整键 p50 约 2.4 µs，相对 \
    键盘输入间隔（数十毫秒）可忽略 ⇒ **不构成优化理由**
  - 尾部代价全部来自 > 20 字符的长整句（p50 ≈ 2.3 ms）——beam 解码的固有工作量，仍远低于交互预算 \
    （~10 ms），且该形态本就少见
  - 因此**不做**参照实现的「增量 / 锁解码缓存」：收益只在长输入路径，风险是该缓存需与解码 arena 的 \
    路径下标生命周期绑定（`decode.rs` 的 `Evaluated::path`），属「改动语义边界」的优化
  - **复核触发条件**：① Android 中低端机实测长整句出现可感卡顿；② 输入长度上限（`MAX_RAW_LENGTH`）\
    放宽；③ 模型从 mobile 换成更大模型。届时再做该缓存，并用本节基准给前后数据
- **维护约定**：改动 decode / 交互路径后跑一遍上面四条命令并与基线比对；**checksum 变化即为行为 \
  变化**，必须查清（差分金样也应报警）
- 词库装载（构造期一次性）另有实测：全量 140–156 ms、仅主表约 17 ms，见 \
  [`../data/README.md`](../data/README.md)「代价」
