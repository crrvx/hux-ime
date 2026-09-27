<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 未完事项 / 待办（open items）

> **本文是活口**：未完事项都落在这里——`[待办]` / `[已登记·不修+理由]` / \
> `[误报·已核实]` 三张表与**待定配置项**（B/C 组）都落在这里。**维护者 = 每批收尾的当班人** \
> （维护者 / AI）：每批收尾勾对一次，不留「文档说未做、代码已做」的条目； \
> 新想法先落本文，实施后移出并同步相关文档。 \
> 分工：做过什么 / 结论是什么（历史与逐批记录）见 [`review-ledger.md`](review-ledger.md)；活规则见 \
> [`design.md`](design.md)，有意偏离上游见 [`upstream-deviations.md`](upstream-deviations.md)。

> 2026-09-21 全仓复核（5 路并行审计 + 人工核实）的已修项见提交 `chore(review)` 三批 \
> 与 `fix(review)`。 \
> **状态前缀**（第 4 批 D1–D6）：`[✅ 已修]` ＝ 已落地且有守护；`[待办]` ＝ 仍未做（含成本估计）； \
> `[已登记·不修+理由]` ＝ 有意不改（理由随条目）；`[误报·已核实]` ＝ 审计结论被实测否掉（见 §3）。 \
> **非审计来源的未闭合项**：发行版打包（PKGBUILD，AUR `fcitx5-hux`）状态见 \
> [`../platform/README.md`](../platform/README.md)；**待定配置项**（B/C 组）见 §4。 \
> 本文把四份总账里**仍活着**的条目提到最前（其余均已 `[✅ 已修]`，本文即其归宿）； \
> 共 **17 条**——`[待办]` 8 / `[已登记·不修+理由]` 7 / \
> `[误报·已核实]` 2。

## 1. `[待办]`（8 条）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| C7（`Group.code`） | 60 万次 `Group.code: Vec<u16>` 小分配 | [待办] | 未做（第 3b 批登记）：需先有基准数据，且要改组查找 / 前缀剪枝 / <br>`collect_chunks` 的取值路径（扁平 `Vec<u16>` + `(start, len)`），<br>收益与风险不匹配，<br>留待性能批 |
| F16 | C++ 壳两处脆弱模式：`applyUpdate` 每次 UI 刷新都重建状态区；<br>`HuxCandidateWord::select` <br>内同步触发回调可能销毁候选对象自身 | [待办] | 未改（当前**无实测故障**，C++ 侧以 `session == nullptr` 早退规避）：<br>需真机 fcitx5 压力验证后再定是否投递到事件循环；本机无 fcitx5 运行环境。<br>**UAF 收尾批补充**：生命周期侧的悬垂风险已加固（候选词弱引用 + `~HuxEngine` <br>清状态区，见「历史纪要」第 6 批），<br>**重入 / 自毁结构未动** |
| M8 | 依赖 / 版本未固定的位置（action 移动标签、`archlinux:latest`、<br>`librime-dev` 版本） | [待办] | **③④ 已实施 / 已注明**：`pacman -Sy` → `-Syu`；<br>`archlinux:latest` **有意不钉**（作业目的即「最新 Lua」）；<br>已在 `goldens/PROVENANCE.md` 注明该类探针金样依赖具体 librime 版本、<br>CI 不重生成（只按 sha 校验）；探针在 CI 只做语法检查。<br>（余见下方 M8 补记） |
| T3.8b | `tools/generators/gen_ngram_golden.lua:74` 用 `("不存在"):sub(1, 3)` 造第 26 个 token | [待办] | 代码**是对的**：Lua `sub` 按字节截出 `不`（`e4b88d`）——金样 29617 行 = 26³+3·26²+10013、<br>`e4b88d` 恰 2104 次；但写法隐晦。改成显式 `"不"` 会改金样，<br>故留到下次重生成金样时一并改（属金样冻结范围，非本轮） |
| K14b | 学习库 materialized 缓存可改为共享分区（原 `learning.rs` 注释里的优化设想） | [待办] | 第 8 批只删了那条未来笔记（原文留在台账）：<br>`Fifo<String, Rc<Materialized>>` 改共享需先有基准数据、<br>收益与风险未知，属性能批 |
| T1.3 | pin/URL 常量硬编码 4+ 处且两套取 pin 机制（`git show` vs <br>`worktree add`） | [待办] | T2.2 / T2.4 已随第 11 批完成；<br>pin/URL 收敛牵动 `ci.yml` 与 4 个生成器，单独一批 |
| `2+3 批残留` | 测试取夹具路径的写法不统一：内联 `join("../../..")` 剩 5 处<br>（`key_sequence_differential.rs` 4 处 +<br>`key_sequence_differential/dump.rs` 1 处），<br>而共享助手 `hux_test_support::repo_path` 已是单点实现 | [待办] | 第 13 批已收掉其余三项：可见性 26 项收紧、<br>`model_status::format_label` 转私有、<br>`scheme_config_with_runtime` 不再从 `crate::engine` 根再导出；<br>`Engine` 字段另见 §2。余项属测试整理，收益仅为一致性 |
| `5 批未做部分` | 第 11 批按风险与文件域切开、留给后续批的项 ——<br>方案·数据：D7 `ReverseLookup`、D10 `trait RankKey`、<br>D11 `StateView`、D12 `expand_range`、D13 `has_complete_candidate`、<br>D16 `LexicalModel` 6 个 pub 字段（须先改集成测试契约）、<br>B10 的 decode 侧选重字节表、B20（10 参 `translate`） | [待办] | D7/D10–D13 触热路径排序与证据分配，须逐项带金样差分验证；<br>D16 受测试契约阻塞；B10/B20 属低成本项，可随任一结构批顺带。<br>**低成本项已随第 13 批收口**：`scheme.rs` 内联测试 635 行迁出、<br>4 条 `paging_action` 用例迁入 `key_binder`、<br>「无时间衰减」括注与新旧语义叙事清理；<br>`punct_shape_comment` 经评估不搬（见 §2） |

- **M8 补记**：**② rust 工具链：有意不钉**（跟随 stable 最新版；CI 用 \
  `dtolnay/rust-toolchain@stable`）——代价是 stable 漂移可能让 `cargo fmt --all --check` / \
  clippy `-D warnings` 无预警变红，届时按当时的稳定版修正即可。 \
  **① action 钉 commit sha 仍待办**：离线无法验证 GitHub 侧可用性，擅自钉死有让 CI \
  无预警变红的实际风险。 \
  **⑤ `cargo-deny`（可选）未做**：属依赖清单 / 许可证 / 公告扫描，收益待评估。

## 2. `[已登记·不修+理由]`（7 条：tiger `A7`/`C3`/`C8`、CI `M12`/`M17`、其余见下表）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| A7 | NaN 语义与 Lua 相反（正常数据不可达） | [已登记·不修+理由] | 第 3b 批：`reward_for_weight` 的 `clamp` 与 `logp` 的 `max` 两处**只注明差异**——<br>入口 `weight > 0.0` 已排除 NaN ⇒ 正常数据不可达、无金样支撑，<br>改行为属投机 |
| C3（跨 crate） | `lexicon::candidate_paths` ↔ `hux_core::scheme::asset_paths`（逐字同逻辑）、<br>`state::{live_input,input_caret}` ↔ <br>`core::session::{live_input,live_caret}`（同构双份）、<br>两套 `BOS/EOS`（`&str` vs `char`） | [已登记·不修+理由] | 第 3b 批**只报告不合并**：三者都牵动契约面或热路径类型（core <br>版已有平台调用者），合并需单独排期；现状无行为漂移（判据已统一），<br>风险是后人改一侧忘另一侧 |
| C8 | 信息项：NaN 语义（见 A7）、<br>`build_edges` 每位置线性扫全部拼写键（449 键 × 段长） | [已登记·不修+理由] | **非缺陷**：NaN 已按 A7 注明；449 键量级的线性扫经评估可接受，<br>报告本身判「仅记录」 |
| M12 | `tools/cases/key_cases.txt` 有无害重复行（`+`、`Shift++a`） | [已登记·不修+理由] | 第 5 批：重复行**有意保留**——删行会改动入库 `key.tsv.gz` <br>的记录数（同一输入两次解析必须一致，金样里各出现两次）；<br>已就地加注释说明，避免后人误读为「覆盖两种解析」 |
| M17 | 信息项：其余安装 / 卸载契约已核实一致 | [已登记·不修+理由] | **无需动作**：报告自述已逐项实测相符（CMake 3 文件、`--purge` 覆盖面、<br>帮助行 `sed` 范围、`data/README.md` 溯源、`docs/config.md` 14 + 3 项、<br>24 份文档 0 破链）；本轮只复跑了其中的金样 sha <br>部分（当时 `verify_golden_shas.py` 61 项通过），<br>未逐项重测 |

| `5 批·punct_shape_comment` | `sound_to_char_shape.rs` 的 `punct_shape_comment` 47 行仍留在门面文件 | [已登记·不修+理由] | 第 13 批评估后不搬：唯一调用点在同文件（标点候选构造），<br>两个子模块（`index` 索引格式 / `graph` 图翻译）都不覆盖该主题，<br>搬动只会放宽可见性或为单函数新开模块 |
| `4 批·Engine 字段` | `platform/fcitx5` 的 `Engine` 有 12 个 `pub(crate)` 字段未收紧 | [已登记·不修+理由] | 第 13 批复核后关闭：12 个字段无一「本可私有」——<br>`crate::abi` / `crate::ui` / `crate::tests` 都是 `crate::engine` 的兄弟，<br>任一使用点存在即不可能私有；收窄只能把 `pub(crate)` 搬到访问器<br>或搬动测试树，收益仅表述、成本为噪音 |

## 3. `[误报·已核实]`（2 条：tiger `B2·子断言`、第 11 批口径修正）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| B2·子断言 | 审计称「本仓把 `ab'1` 切成 abc 段 + raw 段 ⇒ 有 rank-3 候选」 | [误报·已核实] | 第 3b 批实测**两 pin 行完全一致**（`ab'1` 都是 `count=0`）：<br>段结构差异不落在比对面（`preedit` 按设计不比对）；<br>`apostrophe_*_split` 两例逐位通过、<br>无需登记 |
| `11 批口径修正` | 四份总账里被第 11 批实测否掉的四处口径 | [误报·已核实] | ① 平台 `Engine`「41 字段」实为 **26**；<br>② 方案层「`engine.rs` 文档注释挂错函数」不成立（注释本就贴在<br>`refresh_reverse_lookup_aux` 上）；<br>③ B11「`full_before` 两处拼装」实测只 1 处；<br>④ B7「4 处修饰键判据」实测 3 处（`key_matches` 是掩码语义，不合并） |

## 4. 待定配置项（B/C 组）

> 原 `docs/config.md`「待扩展（B/C 组）」（2026-09 由旧 `config-options` 文档迁入）； \
> A 组四项（候选排列、预编辑内容、翻页循环、最短保留码数）已实施。新想法先落本表 \
> （价值 / 现状 / 实现点 / 成本），转入规则见 [`config.md`](config.md)。

**低成本余项（现管线只差暴露）**

| 项 | 现状 | 实现点 | 备注 |
| --- | --- | --- | --- |
| 反查候选上限 | 固定 20 | 方案常量（`sound_to_char_shape::CANDIDATE_LIMIT`）→ 设置 + ABI `int` | 少用 |
| 学习库上限 | 固定 1 万条 / 16 MiB | `learning_store` 常量 → 设置（重启生效） | 少用；「清空学习库」需另做动作，非配置 |
| 候选序号显示 | 随数字直选联动（直选开启才显示 `1`–`9`/`0`） | C++ `setSelectionKey` 条件 → 三态设置 | 少用 |

**B 组（中等成本，可排期）**

- **B1 模型路径**：
  - 自定义 / 禁用 n-gram 模型（Android 分发依赖该能力，模型 APK 走默认目录）；
  - 现仅 `HUX_MODEL` 环境变量、创建时加载（改动需重启）；
  - 做法 = schema `String` → ABI 传路径（空串语义待定）+ 与 `HUX_MODEL` 优先级；
  - 成本 / 风险 = 中 / 低。
- **B2 候选选择键可配置**：
  - 除 Tab/Shift+Tab、Up/Down 外可自定义选字键；
  - 现 host `key_binder` 固定 Tab/Shift+Tab、`selector` 固定 Up/Down（横排）/ ←→（竖排）；
  - 做法 = `HostOptions` 增 `prev/next_candidate_keys`（rime 键名，`KeyList` 可多项）、 \
    `selector` 按列表匹配，理清与翻页键 / 导航键优先级；
  - 成本 / 风险 = 中 / 低。
- **B3 普通候选显示虎码注释**：
  - 学码友好；
  - 现普通解码候选 `comment` 为空、音反查候选为虎码；
  - 做法 = 注释来源（候选路径编码 / 词条虎码）、格式与宽度，**仅展示层换算，不进入排序**；
  - 成本 / 风险 = 中 / 中。
- **B4 码表 / 标点表自定义**：
  - **用户目录同名文件覆盖已可用**（`$XDG_DATA_HOME/fcitx5/hux/` = `~/.local/share/fcitx5/hux/`， \
    放 `tiger_sentence.*.txt` 或 `symbols.yaml` 即生效），无需代码；
  - 路线 = 先补文档（`install.md` / `data/README.md`），UI 指定路径（`String` + 重启）再排期；
  - 成本 = 文档小 / UI 中。

**C 组（高成本，暂缓）**

- **C1 简繁转换**：
  - 输出简 / 繁切换（参照未带，属扩展）；
  - 前置 = OpenCC 级转换表（体积 / 许可 / 来源）、转换挂点（提交文本与候选文本）、 \
    与学习库 / 反查展示的契约；
  - 成本 / 风险 = 高（数据 + 全链路）。
- **C2 用户词 / 自造词**：
  - 词典导入导出与编辑、学习过程可见化；
  - 现只有打分式学习库（LevelDB 同构）、无用户词层；
  - 前置 = 数据结构与迁移、与解码排序 / 学习的关系、两端 UI；
  - 成本 / 风险 = 高。

**明确不做**（登记在案、不再讨论）：

- 早提交概率阈值（share / 证据数）—— 调参危险，参照实现亦未暴露为 UI；
- `memory_profile`（compact / balanced）—— 本实现仅支持 TCSKNM02 mobile 模型；
- `ascii_composer` 系列（Caps / Shift）—— 本实现无内置英文模式。
