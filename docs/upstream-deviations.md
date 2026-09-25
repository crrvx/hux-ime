<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 有意偏离上游（活政策）

> **本文是活政策**：本仓有意偏离上游参照的**全部**登记、依据、可证伪期望值与回归做法。
> 金样记录上游行为、**字节不动**；偏离一律按「本仓逐步期望值 + 期望差异集合 == 实测差异集合」表达。
> 四类各一节：**① 翻页 / 标点遮蔽修复（含用户决定 B）**、**② addon 扩展**、**③ pin 差异**、
> **④ 宿主链交互**（`Ctrl+BackSpace` / `Ctrl+Delete`）——编号沿用原 `docs/refactor.md` §8，既有引用与
> 代码注释按编号即可对应。
> 历史背景（追平批次与历史记录）见 [`review-ledger.md`](review-ledger.md)；
> 结构与契约等活规则见 [`refactor.md`](refactor.md)。

**⚖️ 有意偏离上游 ✅ 已实施**（金样记录上游行为、**字节不动**；偏离项一律按「本仓逐步期望值 + 期望差异集合 ==
实测差异集合」登记在 `crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的 `DEVIATIONS`，
其余金样用例无条件逐位比对。偏离共四类：**上游缺陷修复 + 用户决定的语义强化**（下表 ①）、**addon 扩展**（②）、
**pin 差异**（③）三类入 `DEVIATIONS` 表（可证伪的期望值表）；另有 **④ 宿主链交互**
（`Ctrl+BackSpace`/`Ctrl+Delete` 同义化）——该路径金样覆盖不到，只能由单测守护，见下文 ④。`DEVIATIONS` 是**可证伪的期望值表**：登记项写错、或实际并不偏离，都会让测试失败；
翻页语义强化（下 ①）把 `punct_menu_minus` 也纳入 ①。

## ① 参照缺陷 + 用户决定的语义强化：菜单可见时的 ASCII 翻页键

> 两件事共用同一处判据与同一张偏离表：
> **(a) 上游缺陷修复**——菜单可见时翻页绑定被方案标点分支遮蔽（用户判定为上游缺陷）；
> **(b) 用户决定 B（2026-09，语义强化）**——**菜单可见时翻页键一律拦截**：上翻页不再要求
> 参照 `when: paging` 的「已翻过页」标签，与下翻页同为「菜单可见即成立」。

- **缺陷**（上游 `abad411` `fix(rime): preserve punctuation learning …` 起）：方案处理器在 `context.has_menu()`
  时对**所有**可打印 ASCII 标点先「暂存学习 + 确认组合」再交标点表；而翻页绑定在宿主 `key_binder`
  （缺省 `-`（`when: paging`）/`=`（`when: has_menu`），schema 可绑 `[`/`]`）。于是菜单可见时这些键
  **永远轮不到**翻页绑定；`Page_Up`/`Page_Down`/`Tab` 不经该分支，不受影响。
- **最小复现**：`j a` 后按 `equal` ⇒ 期望下翻一页，上游提交「一=」（金样 `punct_menu_equal` 如实记录）；
  音反查 `` ` z = = `` / `` ` z = = - `` / `` ` z h o = - `` 同理（`nav-page-equal`/`nav-page-minus`/`nav-page-zho`）。
  语义强化 (b) 另有最小复现 `j a` 后按 `minus`：上游未翻页 ⇒ `when: paging` 不成立 ⇒ 提交「一-」，
  本仓按「菜单可见即拦截」上翻页（金样 `punct_menu_minus` 如实记录上游行为、本仓登记期望值）。
- **本仓修法**（判据复用，避免两处条件漂移）：core 抽出**唯一**判据
  `hux_core::host::paging_action(context, options, key_event) -> Option<PagingDir>`
  ——**两侧同前置** `menu_available` = `!ascii_mode && has_menu`：命中 `page_up_keys` ⇒ `Up`、
  命中 `page_down_keys` ⇒ `Down`；`key_binder` 与方案标点分支**共用**它：
  标点分支入口先问一次，判为翻页则**不消费**（不 stage 学习、不确认组合），键落回宿主链执行翻页。
  `ProcessorEnv` 新增 `host_options: &HostOptions`，平台 `TigerScheme` 把与宿主链同一份绑定传进处理器。
- **用户决定 B（2026-09，语义强化）**：参照的 `-` 绑定带 `when: paging`（`key_binder.cc:248-266` 的
  `kWhenPaging` **只看末段 `paging` 标签**，要先翻过页才吃该键）；用户决定改为
  **「菜单可见时翻页键一律拦截」**——上翻页与下翻页同前置，不再看标签。落地：
  `paging_action` 的 `Up` 分支由「命中 `page_up_keys` 且 `has_paging_tag`」改为「命中 `page_up_keys`
  且 `menu_available`」；`paging` 标签的**唯一读取方**（旧的 `kWhenPaging` 判据）随之消失，
  按「不留写了但没人读的字段」删除 `mark_paging`/`has_paging_tag` 与全部写入点
  （金样的比对面不含标签，字节零变化）。
  **已知并接受的代价**：菜单可见时 `-`/`=`/`[`/`]` 不再能作为标点打出（被判为翻页而消费）；
  `ascii_mode` 打开或无菜单时仍照旧落标点/原路径。
- **不受影响的路径**（逐条单测）：无菜单、非标点键、编辑/导航键、`Page_Up`/`Page_Down`、`Tab`/`Shift+Tab`、
  缓冲态标点，以及 **`ascii_mode` 打开时的翻页键**（`menu_available` 不成立 ⇒ 仍确认组合 + 落标点，
  这是本次语义强化的负向对照：core `ascii_mode_paging_keys_fall_through_to_punctuation`、
  `tiger` 的 `processor_menu_paging_keys_bypass_the_punctuation_branch` 后半段、平台用例第 ⑥ 段）。
- **金样策略（字节零变化）**：金样记录的是**上游行为**，故 `goldens/*.tsv.gz` 一个字节都不改；
  偏离用**单一期望值表**表达：`crates/hux-scheme/tiger/tests/key_sequence_differential.rs` 的
  `DEVIATIONS`（一个常量同时服务两份金样 `key_sequence` / `sound_to_char_shape`），每项 = 金样名 +
  **本仓逐步期望值**，并断言「期望 ≠ 金样」的步集合**恰好等于**「实测 ≠ 金样」的步集合
  （既证明确有差异，也防止实现回退成上游后静默通过）；反向断言「登记的偏离已消失」即红。
  文档侧的单一来源是**本文**；金样侧说明见 [`../goldens/README.md`](../goldens/README.md) 的内容表与
  [`../goldens/README.md`](../goldens/README.md) 的「金样不得因本仓有意的行为差异而重生成」。
  登记名必须真实存在；不得静默跳过其它用例：

- **偏离表（三类合并；`steps` 列 = 金样步数，偏离步为「本仓期望 ≠ 金样」的步）**：

  | 类 | 金样 | 偏离用例 | 步数 | 偏离步 | 上游 vs 本仓 |
  |---|---|---|---|---|---|
  | ① | `key_sequence.tsv.gz` | `punct_menu_equal` | 3 | 2（`equal`） | 提交「一=」 vs **翻页**（不提交） |
  | ① | `key_sequence.tsv.gz` | `punct_menu_minus` | 3 | 2（`minus`） | 提交「一-」 vs **上翻页**（首屏归零高亮、不提交）——用户决定 B（上游 `when: paging` 未成立） |
  | ① | `key_sequence.tsv.gz` | `nav_page_home_minus` | 4 | 3（`minus`） | `Page_Up` 停在首页后提交「乙-」 vs **上翻页**（不提交） |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-equal` | 4 | 2,3（`=`） | 上屏「中=」再落「=」 vs **连翻两页** |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-minus` | 5 | 2,3,4（`=`/`=`/`-`） | 同上＋落「-」 vs **翻两页后上翻一页** |
  | ① | `sound_to_char_shape.tsv.gz` | `nav-page-zho` | 6 | 4,5（`=`/`-`） | 上屏「中哦=」再落「-」 vs **下翻一页后上翻一页** |
  | ② | `key_sequence.tsv.gz` | `digit_menu_select` | 3 | 2（`1`） | 数字并入编码 vs **直选当前页候选并上屏** |
  | ③ | `key_sequence.tsv.gz` | `apostrophe_digit_page` | 5 | 4（`Page_Down`） | 末段 abc 单段 ⇒ 消费 `Page_Down` vs 末段 raw ⇒ **不消费** |
  | ③ | `key_sequence.tsv.gz` | `apostrophe_semicolon_page` | 5 | 4（`Page_Down`） | 同上（`'` + `;` 变体） |

  其余用例（含 `nav_page`/`nav_page_big`、`nav-page-keys`、`punct_half_shape_minus`、
  `punct_buffered_period`、`upper_*`，以及新增的 `editor_ctrl_return`/`editor_ctrl_shift_return`/
  `punct_mid_caret_*`/`nav_page_up_home_reset`）**仍逐位一致**。
  `punct_menu_minus`（`j a minus`）在语义强化前与上游逐位一致（`-` 未翻页 ⇒ 双方都落标点），
  强化后**转为偏离项**并登记（首屏 `-` 本仓上翻页、不提交）。
  `nav_page_home_minus`（`a b Page_Up minus`）的偏离在强化前后都成立，但**成因不同**：强化前靠
  「`Page_Up` 停在首页写 `paging` 标签」（上游行为），强化后靠「菜单可见」；上游则因方案处理器
  在确认组合时 `_auto_commit` 立刻提交并清空组合，`-` 落标点提交「乙-」。
- **待上游修复后回归（①）**：上游若采纳下列任一改法**修掉遮蔽缺陷**，随下一批追平时按项重推：
  可以删除的是由遮蔽缺陷直接造成的登记
  （`punct_menu_equal`、`nav-page-equal`/`nav-page-minus`/`nav-page-zho` 的 `=` 步）；
  而 `punct_menu_minus`（首屏 `-`）与 `nav_page_home_minus` 的差异**还取决于上游是否保留
  `when: paging`**：上游保留 ⇒ 本仓的语义强化仍是偏离（保留登记，或按用户新决定回退语义）；
  上游也改成「菜单可见即翻页」⇒ 一并删除登记、恢复无条件逐位比对。
  回归时须重跑 `registry_is_falsifiable` 自校验，并复验平台用例
  `menu_paging_keys_are_not_shadowed_by_the_punctuation_branch` 的 6 条断言（含 `ascii_mode` 负向对照）。

## ② addon 扩展：数字直选（`tiger_sentence_digit_select`，出厂缺省 `true`）

- 上游方案核心没有该选项（数字作为编码字符入串）；本仓按出厂缺省开启 ⇒ 菜单可见时数字直选当前页候选
  （`processor` 的 `select_page_candidate`，走与 `space` 相同的确认/学习链）。金样如实记录上游行为，
  重放按出厂缺省驱动，差异登记为 `AddonExtension`（`digit_menu_select`）。
- 平台侧由状态菜单开关（`hux_engine_option_value + HUX_OPTION_DIGIT_SELECT`）；关闭后不再直选。

## ③ pin 差异：分段常量 `SEGMENTATION_DELIMITER`（`92a0b54` 的 `" '"` vs 主干 `abad411` 的 `" "`）

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
  （只增不改：旧内容是新文件的**严格前缀**；66 例/275 步 → **68 例/285 步**，sha 见 [`../goldens/README.md`](../goldens/README.md)），
  登记为 `BranchPinDelimiter`；`abc_segmentor_splits_after_a_delimiter_before_a_digit` 单测钉住
  `abc_segmentor` 的断段行为（`'`+数字/`;` 断开、`'`+首字母单段）。
  负向对照：把常量改回 `" "` ⇒ 两条登记项报「登记的偏离已消失」并失败。
- **待上游并入后的回归做法**：`feat/reverse-lookup` 并入 main 后，随下一批追平**重生成 `key_sequence` 金样**
  （两 pin 合一，`apostrophe_*_page` 记录的行为与本仓一致）⇒ 删除这两条登记项，恢复无条件逐位比对；
  届时 `tools/cases/key_sequence_cases.txt` 的偏离注释同步改写。
  （另注：尖端还有 `punct_segmentor` 把反查段里的 `;` 直接落成全角「；」——本仓的标点由宿主表处理、
  不实现该分段器，属**另一处**已知范围差异，与本项 delimiter 差异无关。）
- **撇号音节切分（既有口径）与上游 librime 依赖**：`92a0b54` 的「按 `speller/delimiter` 切分音节」
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

## ④ 宿主链交互：`Ctrl+BackSpace` / `Ctrl+Delete` 与不带修饰者同义（用户要求；金样覆盖不到）

- **参照**（librime `editor.cc` @ `33e78140`）：`{XK_BackSpace, kControlMask}` = `Editor::BackToPreviousSyllable`
  （按音节回退）、`{XK_Delete, kControlMask}` = `Editor::DeleteCandidate`（删除高亮候选）。
- **本仓**：按要求**取消这两个交互**——`Ctrl+BackSpace` ≡ `BackSpace`、`Ctrl+Delete` ≡ `Delete`
  （`crates/hux-core/src/host.rs` 的 match 臂已把修饰位并入普通臂）。
- **为何不入 `DEVIATIONS` 表**：这两个键在真机路径上会先被方案 `processor` 消费，探针用例清单也无 Ctrl 变体，
  金样与差分**都覆盖不到**宿主链 ⇒ 只能由单测守护：
  `ctrl_backspace_and_ctrl_delete_match_their_plain_variants`——对同一初始状态分别按「带 Ctrl」与「不带 Ctrl」，
  断言可观测状态指纹（输入 / 光标 / 组合段与选中态 / 菜单 / 已上屏文本）**逐字段相等**。
- **回归做法**：若要恢复参照语义，把 `host.rs` 中并入的 `K_CONTROL_MASK` 拆回独立分支、
  恢复 `BackToPreviousSyllable`/`DeleteCandidate`（后者还需 `selected_index` 能表达参照的 `-1`），并改该单测；
  **不需要动任何金样**。
- 连带结论：`selected_index` 无法表达 `-1`、`Ctrl+Delete` 退化为吞键等条目（[`review-ledger.md`](review-ledger.md) §5.1；`selected_index` 无法表达 `-1`、`Ctrl+Delete` 退化为吞键）随本决定**关闭**——
  不再需要「删除候选」通道。
