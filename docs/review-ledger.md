<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 复核台账：历史与逐批记录（review ledger）

> **本文是历史留档**：§1「历史纪要」把迁移映射 / 批次 / 上游追平 / 逐批整改 / 四份审计总账压成逐批一行（结论 + 提交号）。 \
> **§0「未闭合项」是活口**——全仓唯一记录未完事项（`[待办]` / `[已登记·不修+理由]` / `[误报·已核实]` / 待定配置项）之处。 \
> 活规则见 [`design.md`](design.md)，有意偏离上游见 [`upstream-deviations.md`](upstream-deviations.md)；本文只记「做过什么 / 结论是什么」，不承载规则本身。

---

## 0. 未闭合项（活口）

> 2026-09-21 全仓复核（5 路并行审计 + 人工核实）的已修项见提交 `chore(review)` 三批与 `fix(review)`。 \
> **状态前缀**（第 4 批 D1–D6）：`[✅ 已修]` ＝ 已落地且有守护；`[待办]` ＝ 仍未做（含成本估计）； \
> `[已登记·不修+理由]` ＝ 有意不改（理由随条目）；`[误报·已核实]` ＝ 审计结论被实测否掉（本节 2 条，见 §0.3）。 \
> 每批收尾勾对一次，不留「文档说未做、代码已做」的条目。 **非审计来源的未闭合项**： \
> 发行版打包（PKGBUILD，AUR `fcitx5-hux`）状态见 [`../platform/README.md`](../platform/README.md)；**待定配置项**（B/C 组）见 §0.4。 \
> 本节把四份总账里**仍活着**的条目提到最前（其余均已 `[✅ 已修]`，本节即其归宿）； \
> 共 **18 条**——`[待办]` 11 / `[已登记·不修+理由]` 5 /  \
> `[误报·已核实]` 2。

### 0.1 `[待办]`（11 条）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| C7（`Group.code`） | 60 万次 `Group.code: Vec<u16>` 小分配 | [待办] | 未做（第 3b 批登记）：需先有基准数据，且要改组查找 / 前缀剪枝 / <br>`collect_chunks` 的取值路径（扁平 `Vec<u16>` + `(start, len)`），<br>收益与风险不匹配，<br>留待性能批 |
| F16 | C++ 壳两处脆弱模式：`applyUpdate` 每次 UI 刷新都重建状态区；<br>`HuxCandidateWord::select` <br>内同步触发回调可能销毁候选对象自身 | [待办] | 未改（当前**无实测故障**，C++ 侧以 `session == nullptr` 早退规避）：<br>需真机 fcitx5 压力验证后再定是否投递到事件循环；本机无 fcitx5 运行环境。<br>**UAF 收尾批补充**：生命周期侧的悬垂风险已加固（候选词弱引用 + `~HuxEngine` <br>清状态区，见「历史纪要」第 6 批），<br>**重入 / 自毁结构未动** |
| M8 | 依赖 / 版本未固定的位置（action 移动标签、`archlinux:latest`、<br>`librime-dev` 版本） | [待办] | **③④ 已实施 / 已注明**：`pacman -Sy` → `-Syu`；<br>`archlinux:latest` **有意不钉**（作业目的即「最新 Lua」）；<br>已在 `goldens/README.md` 注明 CI 的 librime 版本可不同、仅做语法检查。<br>（余见下方 M8 补记） |
| T3.8b | `tools/generators/gen_ngram_golden.lua:99` 用 `("不存在"):sub(1, 3)` 造第 26 个 token | [待办] | 代码**是对的**：Lua `sub` 按字节截出 `不`（`e4b88d`）——金样 29617 行 = 26³+3·26²+10013、<br>`e4b88d` 恰 2104 次；但写法隐晦。改成显式 `"不"` 会改金样，<br>故留到下次重生成金样时一并改（属金样冻结范围，非本轮） |
| T3.1b | `tools/checks/check_data_manifest.sh:32` 用 `sed` 去行尾空白，与 `data/MANIFEST` 的「行首尾不留空白」约定不一致 | [待办] | 这种行能过守卫、却会让 `install.sh` 的整行匹配直接 die ⇒ 应让守卫判错；<br>属行为变更（既有可能让现存清单变红，也牵动装/卸契约），留待下一批 |
| K14b | 学习库 materialized 缓存可改为共享分区（原 `learning.rs` 注释里的优化设想） | [待办] | 第 8 批只删了那条未来笔记（原文留在台账）：<br>`Fifo<String, Rc<Materialized>>` 改共享需先有基准数据、<br>收益与风险未知，属性能批 |
| T1.3 | pin/URL 常量硬编码 4+ 处且两套取 pin 机制（`git show` vs <br>`worktree add`） | [待办] | T2.2 / T2.4 已随第 11 批完成；<br>pin/URL 收敛牵动 `ci.yml` 与 4 个生成器，单独一批 |
| `2+3 批残留` | `lexicon` / `sound_to_char_shape` 的文件名与资产常量仍 `pub`、<br>`model_status::format_label` 只被本文件单测用、夹具路径字面量两份、<br>`Engine` 的 `pub(crate)` 字段与 `scheme_config_with_runtime` 未收紧 | [待办] | 第 11 批已完成「`Engine` 装配置于一处」（`Assembly`/`Diagnostics`）；<br>余项需先确认无 crate 外消费者，留待结构批 |
| `4 批残留` | `platform/fcitx5/src/tests.rs` 仍留 4 个只服务单一主题的助手<br>（`key_list`/`ffi_engine`/`reverse_lookup_character_dirs`/<br>`abi_enum_members`）；`key_routing`/`ffi_mapping`/`status` 各有机械归纳的存疑用例；<br>子文件未加 `//!`、`mod` 声明放父文件末尾 | [待办] | 注解子批（F13/F14）已在第 11 批收工，但本条未随其处理；<br>留待低成本清理批（可与第 12 批同批） |
| `5 批未做部分` | 第 11 批按风险与文件域切开、留给后续批的项 ——<br>方案·数据：D7 `ReverseLookup`、D10 `trait RankKey`、<br>D11 `StateView`、D12 `expand_range`、D13 `has_complete_candidate`、<br>D16 `LexicalModel` 6 个 pub 字段（须先改集成测试契约）、<br>B10 的 decode 侧选重字节表、B20（10 参 `translate`）；<br>低成本项：`scheme.rs` 内联测试约 635 行、`punct_shape_comment`（46 行）、<br>测试体内两处「无时间衰减」字样、四处新旧语义对比叙事、<br>4 条 `paging_action` 用例仍留 `selector.rs` | [待办] | D7/D10–D13 触热路径排序与证据分配，须逐项带金样差分验证；<br>D16 受测试契约阻塞；<br>B10/B20/内联测试属低成本项，可随任一结构批顺带 |
| 第 12 批（文档重排） | 本轮全仓审计剩下的最后一批：29 处超 100 列、<br>拆 `open-items.md`/`REGENERATE.md`/`PROVENANCE.md`、表格单元超 80 列 | [待办] | 未获批准：逐条明细在 captain 的审计工作稿（`_tmp/audit/`，不入库）；<br>批准后按文档批派工 |

- **M8 补记**：**② rust 工具链：有意不钉**（跟随 stable 最新版；CI 用 `dtolnay/rust-toolchain@stable`）—— \
  代价是 stable 漂移可能让 `cargo fmt --all --check` / clippy `-D warnings` 无预警变红， \
  届时按当时的稳定版修正即可。**① action 钉 commit sha 仍待办**：离线无法验证 GitHub 侧可用性， \
  擅自钉死有让 CI  \
  无预警变红的实际风险

### 0.2 `[已登记·不修+理由]`（5 条：tiger `A7`/`C3`（跨 crate）/`C8`、文档工具CI `M12`/`M17`）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| A7 | NaN 语义与 Lua 相反（正常数据不可达） | [已登记·不修+理由] | 第 3b 批：`reward_for_weight` 的 `clamp` 与 `logp` 的 `max` 两处**只注明差异**——<br>入口 `weight > 0.0` 已排除 NaN ⇒ 正常数据不可达、无金样支撑，<br>改行为属投机 |
| C3（跨 crate） | `lexicon::candidate_paths` ↔ `hux_core::scheme::asset_paths`（逐字同逻辑）、<br>`state::{live_input,input_caret}` ↔ <br>`core::session::{live_input,live_caret}`（同构双份）、<br>两套 `BOS/EOS`（`&str` vs `char`） | [已登记·不修+理由] | 第 3b 批**只报告不合并**：三者都牵动契约面或热路径类型（core <br>版已有平台调用者），合并需单独排期；现状无行为漂移（判据已统一），<br>风险是后人改一侧忘另一侧 |
| C8 | 信息项：NaN 语义（见 A7）、<br>`build_edges` 每位置线性扫全部拼写键（449 键 × 段长） | [已登记·不修+理由] | **非缺陷**：NaN 已按 A7 注明；449 键量级的线性扫经评估可接受，<br>报告本身判「仅记录」 |
| M12 | `tools/cases/key_cases.txt` 有无害重复行（`+`、`Shift++a`） | [已登记·不修+理由] | 第 5 批：重复行**有意保留**——删行会改动入库 `key.tsv.gz` <br>的记录数（同一输入两次解析必须一致，金样里各出现两次）；<br>已就地加注释说明，避免后人误读为「覆盖两种解析」 |
| M17 | 信息项：其余安装 / 卸载契约已核实一致 | [已登记·不修+理由] | **无需动作**：报告自述已逐项实测相符（CMake 3 文件、`--purge` 覆盖面、<br>帮助行 `sed` 范围、`data/README.md` 溯源、`docs/config.md` 14 + 3 项、<br>24 份文档 0 破链）；本轮只复跑了其中的金样 sha <br>部分（`verify_golden_shas.py` 61 项通过），<br>未逐项重测 |

### 0.3 `[误报·已核实]`（2 条：tiger `B2·子断言`、第 11 批口径修正）

| 编号 | 一句话问题 | 状态 | 归宿（提交 / 批次 · 不修理由 · 待办成本） |
|---|---|---|---|
| B2·子断言 | 审计称「本仓把 `ab'1` 切成 abc 段 + raw 段 ⇒ 有 rank-3 候选」 | [误报·已核实] | 第 3b 批实测**两 pin 行完全一致**（`ab'1` 都是 `count=0`）：<br>段结构差异不落在比对面（`preedit` 按设计不比对）；<br>`apostrophe_*_split` 两例逐位通过、<br>无需登记 |
| `11 批口径修正` | 四份总账里被第 11 批实测否掉的四处口径 | [误报·已核实] | ① 平台 `Engine`「41 字段」实为 **26**；<br>② 方案层「`engine.rs` 文档注释挂错函数」不成立（注释本就贴在<br>`refresh_reverse_lookup_aux` 上）；<br>③ B11「`full_before` 两处拼装」实测只 1 处；<br>④ B7「4 处修饰键判据」实测 3 处（`key_matches` 是掩码语义，不合并） |

### 0.4 待定配置项（B/C 组）

> 原 `docs/config.md`「待扩展（B/C 组）」（来自 `docs/config-options.md`，2026-09 迁入）； \
> A 组四项（候选排列、预编辑内容、翻页循环、最短保留码数）已实施。记录与转入规则随 [`config.md`](config.md)： \
> 新想法先落本表（价值 / 现状 / 实现点 / 成本），实施后从本表移除并同步该文与测试。

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
  - 路线 = 先补文档（`usage.md` / `data/README.md`），UI 指定路径（`String` + 重启）再排期；
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

**明确不做**：早提交概率阈值（share / 证据数，调参危险、参照亦未暴露为 UI）；`memory_profile`（compact /  \
balanced，本实现仅支持 TCSKNM02 mobile 模型）；`ascii_composer` 系列（Caps / Shift，无内置英文模式）。

---

## 1. 历史纪要（逐批一行；原 §1 迁移映射 / §2 批次 / §3 上游追平 / §4 逐批整改 / §5 四份审计总账）

| 批次 / 主题 | 结论 | 提交 / pin |
| --- | --- | --- |
| P0–P1 文档评审、机械解耦（`hux-addon` 拆模块、`interaction.rs` 拆目录） | 行为 / API 不变，用例 + 金样全绿 | — |
| P2 平台承接环境耦合（数据目录 / 时钟 / 状态日志），core 清 env / XDG 硬编码 | 桌面与 Android 路径均由平台构造 | — |
| P3 拆 crate 与 `platform/`（`hux-cfg` / `hux-ffi` / `platform/fcitx5` / `platform/linux` + 骨架 README） | workspace 编译通过、金样全绿 | — |
| P4 `hux_core::scheme` 最小契约 + `hux-scheme/tiger` 物理拆分（P4a 去回边 / P4b 迁模块 / P4c 契约注入 / 自审收尾） | 契约落地、不实现新方案；`OptionIds` 等固定契约随后由「① 契约去虎码语义」删除 | — |
| P5 测试正式化（`hux-test-support` 收编两份 `tests/common/`、单元 / 集成分离、CI 分层） | CI 全绿、全仓无 `mod.rs` | — |
| P6 性能：先测后优化收口 | **不做**增量解码缓存——打字路径已是微秒级（1–5 字符 p95 ≤ 3.4 µs），尾部仅来自 >20 字符长整句 | — |
| 迁移阶段与上游追平 B1–B5（`8b615235` → `abad411`，反查支线 → `92a0b54`） | spike 差分 fixture 29,617 行 + 真实模型 62,777 行逐位一致、吞吐约 82×；B1–B5 全落地（早提交权重拆分、跨来源融合 / Direct 序、证据分配优化、等级化纠错去时间衰减、竞争边界前瞻），金样除头部 pin 外逐字节不变 | `abad411` / `92a0b54` |
| 第 1 批：core F1（`unframe` 非字符边界 panic）、平台 F1/F2/F3/F12（角色守护 / ABI 哨兵 / 标识符中性化） | 已修（`unframe` 改 `get(a..b)?`，坏帧跳过并进诊断） | `8336316a` / `a640141d` / `80c965a2` |
| 第 2 批：core F2–F12 真缺陷（`Page_Up` 首页标签、`Ctrl(+Shift)+Return`、caret 标点、契约袋错误通道、`highlight`、死码） | 已修（F2 / F11 判据后被「用户决定 B」改写为「菜单可见」） | `a792e79d` + 金样 `d94face3` |
| 第 3a 批：tiger A1/A2/A4/B1/B3/B8（反查索引上界校验、偏离登记升期望值表、出厂缺省重放） | 已修（恒真自校验换 `DEVIATIONS` 期望值表） | `56fca679` |
| 第 3b 批：tiger A3/A5–A8/B2/B4–B6/C1–C5/C7 读音串（+ 6 条已知遗留收口） | 已修；A7（NaN 语义）/ C3（跨 crate 重复）/ C8（信息项）登记不修；遗留⑤⑥仍成立但为空操作 / 两侧同构 | `f6372f37` / `63b8d74e` / `84e3beba` / `ff9217e1` |
| 第 4 批：文档 D1–D14、工具 M1–M17、金样机制（内部头部校验 / 生成器原子写）、CI 守卫、平台 F5/F6/F8/F15 | 已修；M12 / M17 登记不修、M8 留待办（见 §0） | `37f27882` / `9774b639` / `d2f771cb` / `17f7bf2a` |
| 第 5 批：遗留②③ 补金样（学习 × 早提交组合、Tab 锁真机探针）+ cfg / 平台 / 工具清尾 | 已修（`verify_golden_shas.py` 61 项通过） | `5304bd35` |
| 第 6 批：文档肃清整合 + 翻页语义按「决定 B」强化 + UAF 收尾 | 已修；`paging` 标签结构性退役；UAF：`HuxSession` 析构契约源码级核实 + 真机核对通过 ⇒ 会话路径无 UAF，候选词自毁压力验证仍未做（见 §0.1 F16） | `4cd0bcaa` / `e7b6054b` / `884e9798` |
| ① 契约去虎码语义（跨 4 crate） | 已修：`OptionIds` / 固定字段 `SchemeConfig` 删除，改「方案自报角色声明 + 通用键值袋」；线上字符串保留（不破上游互通 / 老配置 / ABI 布局） | — |
| 第 7 批：C6 公开 API 去 `hashbrown`（不透明 `Map` / `Set`）+ F10.1 学习索引断言 | 已修：迭代序与哈希器语义逐位不变；负向对照实测失败 | `795e31cf` / `10a2de6f` |
| 配置页保存不落盘（用户报告「提前上屏至预编辑无效」） | 已修：`setConfig` 增 `safeSaveAsIni` + `reloadConfig()`（值不再被 `options.yaml` 静默压回） | — |
| 四份只读审计总账（85 条发现 / 96 行） | `[✅ 已修]` 87 / `[待办]` 3 / `[已登记·不修+理由]` 5 / `[误报·已核实]` 1；逐条明细与「修法 + 守卫 + 负向对照」随本次精简删除，活口见 §0 | — |
| 第 8 批（本轮全仓审计的 0+1 批）：ABI 枚举取值具名化 + 死代码 / 过期记录清理 | 已修：`hux_abi.h` 增 `HUX_CANDIDATE_LAYOUT_*` / `HUX_PREEDIT_MODE_*`，<br>C++ 壳改用宏 + 两条 `static_assert`，`abi.rs` 只给非默认档起名，<br>并加取值守卫用例（`tests.rs` 的 `abi_enum_members` 按前缀分段取枚举）；<br>删死码 K8/K10/K11/K13/K14/K15、B16–B19、D18–D20、T3.1–T3.8、<br>平台 5 项 + `engine.rs:265` + `ui.rs:93-97`、`docs/usage.md:27` 与 5 个 README；<br>K12 复核后**完成**（再导出删除、`store::LEGACY_FILE` 降为模块私有）；<br>用例 391 → 390（删两个、加 `option_value_enums_match_the_abi_header`；<br>「学习分不随时间衰减」失去钉桩，语义改由「方法已不存在」保证）；<br>保留待办 T3.8b / T3.1b / K14b（见 §0.1） | `19423fe1`（ABI）+ `711acd6e`（清理） |
| 第 9 批（审计整改 2+3 批）：常量单点化 + 公开面收缩 | 已修：tiger D1/D3/D4/D6/D8/D17（`has_selection_suffix` 唯一实现 + <br>`early_commit` 再导出、`LEXICAL_FILE`、magic 判定下沉 `ngram::detect_format`、<br>`added == 4` 具名、`0.99999` 互指注释）、B3/B5/B8/B9/B15（`interaction` 七条 <br>glob 换显式导出、kind 字面量单点化、`CANDIDATE_LIMIT` 与 `state` 再导出清理、<br>`lexicon::{rebuild, reward_for_weight}` 降私有）；cfg K1/K7（`roles` 回用 <br>`collections::Map` 并补 `PartialEq`/`Eq`、去 `hashbrown` 依赖、两个 <br>`option_defaults` 改名 `builtin_option_defaults`/`session_option_defaults`）；<br>工具 T1.4/T1.5/T2.1（新 `_hashutil.py` / `_common.py`，哈希三口径与 <br>`check_resources` 累计 `fail` 语义各自保留）；平台 Engine 装配口径单点化 <br>（`option_keys`/`options_store`/`open_learning`，`redeploy` 语义不变）+ <br>26 字段 / 17 方法可见性收紧；captain 补修接缝 `decode.rs:403/432` 与 <br>`docs/design.md:100` 过期引用；用例 390 → 391 | `1ba5c131`（数据层）+ `f089c4fb`（交互层）<br>+ 本行提交（配置·工具·平台 + 台账） |
| 第 10 批（审计整改第 4 批）：测试分层与跨层去重 | 已修：平台 F2（`new_with_dirs` 去 `#[cfg(test)]`、加 <br>`#[cfg_attr(not(test), allow(dead_code))]`，`lib.rs` 13 行 test 再导出删除）、<br>F1（8 个源码文本守卫迁为集成测试 `tests/host_contract.rs`）、F9（3 个裸 ABI <br>入口补 6 条契约用例）、F6（57 处手搓仓库路径改 `hux_test_support::repo_path`）、<br>F4（`src/tests.rs` 3101 行 / 87 用例拆成 266 行父文件 + 12 个主题子文件；<br>父模块私有助手靠 `use super::*;` 继承，零可见性放宽）；<br>tiger F5（`interaction/tests.rs` 拆 8 个主题文件，<br>**实测 72 用例而非审计说的 73**）、<br>F3（删 `#[cfg(test)] candidate_paths`）、<br>F10/F12（跨层 1:1 重复改内核侧深度断言）；<br>core F8（单测按生产侧分 5 节并重排 7 条）、F11（4 条标点用例并为表驱动）、<br>F7（cfg 8 处改 `hux_test_support::temp_dir`）；<br>用例 391 → 393；余项 `F13/F14` 与 `4 批残留` 见 §0.1 | `ac6db77b`（平台层）+ `24775088`（方案层）<br>+ 本行提交（内核·cfg 层 + 台账） |
| 第 11 批（审计整改第 5 批）：结构大改 | 已修：<br>方案·数据 A —— `decode.rs` 2923 行拆 `decode/`（beam 1349 / fusion 136 / <br>evidence 320 / reachability 224 / tests 716，门面 376 行）、<br>D2 哨兵码点由 `ngram` 导出（`BOS_CHAR`/`EOS_CHAR`/`BOS_CODE`/`EOS_CODE`）、<br>D9 基准样板移 `hux-test-support`、D14 `State` 三处构造合一、<br>D15 第二个 `impl Decoder` 落位（D17 前批已办）；<br>方案·交互 —— B11 `processor()` 594 行 → `process_key_event()` 92 行 + <br>10 个 `handle_*` + `KeyDispatch`、B2 码注释下沉 `lexicon`（断反向成环）、<br>B4 字反查判据收进 `char_to_sound_shape`、B6 `min_retained` 去 `Option`、<br>B7/B10/B12/B13/B14/B15/B16；`sound_to_char_shape.rs` 1269 → 140 + <br>index 313 + graph 346 + tests 510、`scheme.rs` 1373 → 1068 + assets 134 + <br>config 200；<br>内核 —— `host.rs` 1770 → 132 + `host/` 七文件 1856 行、<br>K3 合并 Confirm 双分支、K4/K9 清历史注释（K5/K14 前批已办）；<br>平台 —— `engine.rs` 1060 → 742 + `engine/{assembly,config,diagnostics}.rs`、<br>K2 `HUX_OPTIONS_FIELDS` 提 `pub const` + 19 项往返用例<br>（`redeploy` 先释放学习库句柄的修正保留）；<br>工具链 —— 新增 `lib/golden_fixture.sh` / `lib/lua_util.lua` / <br>`regen_goldens.sh`（T1.1/T1.2/T1.6/T1.7/T2.2–T2.7），`ci.yml` 489 → 349，<br>全部金样与生成物逐字节不变；<br>根脚本 —— 抽 `tools/scripts/lib.sh`，uninstall 16 平行数组 × 6 触点改落点表，<br>`--help` / `--dry-run` 输出逐字节不变；<br>注解 —— F13/F14 最差文件（6 + 8 文件）与遗留 5 文件（88 条 message + <br>7 条 `///`）补齐，共 177/177 断言带上下文、72/72 测试有 `///`；<br>用例 393 不变（平台 +1 / 方案层 −1）；全量基线 12 条命令全绿 | `e5f66680`（<br>工具链）<br>+ `f2b47be2`（根脚本）<br>+ `343011d9`（平台层）<br>+ `4e683632`（内核·cfg 层）<br>+ `b9033a50`（方案·数据层）<br>+ `7bdc028b`（方案·交互层）<br>+ 本行提交（台账） |
