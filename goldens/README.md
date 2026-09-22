<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：差分金样

金样由参照实现（[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 的 Lua 核心）生成，Rust 侧逐位重放比对
（`crates/hux-scheme/tiger/tests/*_differential.rs` 与 `crates/hux-core/tests/*_differential.rs`）。

> **本文**：金样是什么 / 内容表 / transcript 格式 / 校验入口 / 规则。
> **重新生成命令全表、来源与校验和（sha 表）** → [`regenerate.md`](regenerate.md)。

## 内容

| 文件 | 用途 | 规模 |
|---|---|---|
| `ngram_fixture.bin` | ngram 确定性小模型（参照仓库 `tools/model_fixture.lua` 生成） | 17,480 B |
| `ngram_fixture.tsv.gz` | ngram 金样：`logp`/`obs`/`status`/`cfg`/`trim` | 29617 行 |
| `lexicon/` | 码表数据夹具（codes / char_ranks / full_code_whitelist / supplement） | 4 文件 |
| `lexicon.tsv.gz` | lexicon 金样：`status`/`lengths`/`probe`/`limit`/`supp` | 18357 行 |
| `lexicon_missing.tsv.gz` | 数据缺失路径金样 | 6 行 |
| `lexicon_variants/` + `lexicon_variants.tsv.gz` | 解析边界数据（BOM/CRLF/大写码/重复/非法行/白名单豁免/高频过滤） | 27 条 |
| `lexicon_codes_only/` + `lexicon_codes_only.tsv.gz` | 仅码表（无字频/白名单/补充） | 13 条 |
| `decode.tsv.gz` | decode 金样（无模型）：`decode`/`result` | 1980 行 |
| `decode_model.tsv.gz` | decode 金样（fixture 模型，抽样） | 351 行 |
| `decode_rank_first.tsv.gz` | decode 金样（fixture 模型 + 关闭单字重码，抽样） | 330 行 |
| `decode_evidence.tsv.gz` | 早提交证据金样（无模型；含 `has_complete_candidate` 的 `complete` 用例） | 12241 行 |
| `decode_evidence_model.tsv.gz` | 早提交证据金样（fixture 模型，抽样；同上） | 2861 行 |
| `learning.tsv.gz` | 学习金样：`hash`/`score`/`prefix`/`confirmed`/`reward`/成熟度/`diff`/融合偏好/人工纠错等级/日志编码 | 10289 行 |
| `decode_learning.tsv.gz` | 解码接入学习（无模型；含一条成对融合偏好） | 1988 行 |
| `decode_learning_model.tsv.gz` | 解码接入学习（fixture 模型，抽样；同上） | 359 行 |
| `decode_learning_evidence.tsv.gz` | 早提交证据 **+ 学习接入**（`--early-commit 1 --required 1 --learning 1`，无模型；遗留②：学习 × 证据抑制的交互——`learning=1 && truncated=1` 的截断池与 `share`/`base_share` 双权重） | 12257 行 |
| `key.tsv.gz` | 键名/键事件金样（librime 探针）：`name`/`repr`/`parse`/`modifier` | 5136 行（5132 条记录 + 4 行头部） |
| `key_sequence.tsv.gz` | 键序列金样（真 librime 探针；主干 pin）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 68 例 / 285 步（含空码自动上屏、编辑/导航键、标点表、大写字母直接提交、`Return` 修饰键变体（`Ctrl(+Shift)+Return`）、标点表 caret 语义、selector 首页上翻、数字直选、撇号分段；`punct_menu_equal`/`punct_menu_minus`/`nav_page_home_minus` 记录**上游行为**「菜单可见的 ASCII 标点先确认组合再交标点表」——三例都因本仓**有意偏离**在差分测试中按期望值登记：`punct_menu_equal` 是上游缺陷（翻页绑定被标点分支遮蔽）修复，`punct_menu_minus` 是**用户决定的语义强化**（菜单可见时上翻页键一律拦截、不再要求 `when: paging` 的「已翻过页」标签；代价：菜单可见时这些键打不出标点——见 [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md) ①），`nav_page_home_minus` 两者兼有；`digit_menu_select`（addon 数字直选）、`apostrophe_digit_page`/`apostrophe_semicolon_page`（本仓分段常量追踪反查分支尖端 `92a0b54` 的 `delimiter: " '"`）同样按期望值登记，见 [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)） |
| `key_sequence/` | 键序列夹具（合成码表 + `symbols.yaml`＝参照 pin 同文件；探针与 Rust 重放共用；发布默认见 `data/symbols.yaml`） | 2 文件 |
| `key_sequence_tab.tsv.gz` | Tab 锁路径键序列金样（真 librime 探针；主干 pin；**夹具 `tab_learning: true`** ⇒ 参照学习库就绪、走 `learned.store.db` 的 Tab 基线捕获分支——遗留③；逐步同 `key_sequence` 字段） | 8 例 / 44 步（`tab_lock`/`tab_confirm_space`/`tab_confirm_buffer`/`tab_buffer_space`/`nav_tab_binder`/`tab_lock_left`/`tab_lock_backspace`/`tab_lock_up_down`） |
| `key_sequence_tab/` | Tab 金样夹具（`symbols.yaml` 与合成码表与 `key_sequence/` **逐字节相同**，另含 `tiger_sentence.custom.yaml` 把 `tab_learning: true` 这一**条件本身**入库） | 3 文件 |
| `sound_to_char_shape.tsv.gz` | 音反查金样（真 librime 探针；反查分支尖端 pin，已含主干）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 31 例 / 164 步（裸前缀标点候选、缩写/全拼剪枝、多音节词、Page 键翻页、导航/退格/Escape/上屏、数字直选与分号惰性、撇号保留；`nav-page-equal`/`nav-page-minus`/`nav-page-zho` 记录**上游行为**「`=`/`-` 被标点分支遮蔽」，因上游缺陷被本仓**有意偏离**、在差分测试中登记跳过，其余（含 `nav-page-keys`）逐位一致，见 [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)） |
| `sound_to_char_shape/` | 音反查夹具（小 `PY_c.dict.yaml` + 合成码表 + `symbols.yaml` + `gen_pinyin_index.py` 生成的 `tiger_sentence.pinyin.bin`；探针与 Rust 重放共用） | 4 文件 + 生成物 |
| `lexical.tsv.gz` | 词先验金样（TCSLEX01 读取/Bloom/打分；真实位图 + 码表语料） | 753 行 |
| `local/`（不入库） | 真实模型抽样金样（224 MB 模型） | 62,777 条 |

## transcript 格式（TSV，`#` 注释，`-` 表示空串，字符串为 UTF-8 字节十六进制）

```
# ngram
bytes  <file_size>
logp   <hex a> <hex b> <hex c> <0x hi|lo bits>   # f64 位模式
obs    <hex a> <hex b> <0|1>
cfg    <page> <context> <bigram> <index>          # 执行 configure_cache
trim / close
status <k=v>...                                    # cache_status 规范快照

# lexicon
status  <k=v>...                      # data_status 规范快照
lengths <n,n,...>
probe   <hex code> <hex(text):rank:optimal,...|->
limit   <n>                           # 执行 apply_high_freq_limit
supp    count=<n> error=<0|1>

# decode（冷路径；include_early_commit=false，未接入学习）
decode <hex input> count=<n> learning=<0|1> truncated=<0|1> required=<hex prefix|->
result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank> <edge_count> <bits supplement_score> <bits learning_score> <bits early_commit_confidence_score>
# decode + 早提交证据（--early-commit 1）
evidence <hex proposal> <bits proposal_share> nit= mit= nlc= trunc= prefixes= raws=
prefix <hex text> <raw_length> <bits share> <bits base_share> <bits boundary_share> <closed> <chars>
rawlen <hex text> <raw_length>
# decode + 学习接入（--learning 1）
learningsetup <now> <hex mode> <n>   # 后接 n 条 levent
levent <time> <hex mode> <hex code> <hex text> <hex ctx>
                                     # 其中一条是成对融合偏好（mode 为 `fusion-v1|…`、
                                     # text 为 `D`/`C`），用于让跨来源融合排序在解码金样里可见

# learning（纯计算；见生成器头部注释的完整字段表）
hash <hex text> <value>
corpus / event / index / confirmed / codes / score / prefix / trim
chain / node / reward / maturity / contribution / diffcase / diffpath / diff / diffevent
fusionmode / paircode / fusion / fusionnone / fusionevent
journalrecords / journalrecord / journalevents / journalevent
#   （`reinforce*` 记录随参照 `7b220ce` 删除 `M.reinforce` 一并移除；
#    `levels_*` 索引守护人工纠错等级的 +2/级、10 级封顶与「无时间衰减」）

# key（librime 探针）
name <keyval> <name|->
repr <keyval> <modifier> <repr>
parse <repr> <ok|bad> <keycode> <modifier> <repr>
modifier <index> <name|->

# key_sequence / sound_to_char_shape（真 librime 探针）
case <case> <options>
step <case> <index> <repr> <consumed 0/1> <input> <caret> <commit> <preedit>
     <page> <highlight> <candidate_count> <candidates> <comments>
```

## 校验

```sh
cargo test --workspace        # 全部差分（本地 sample 缺失自动跳过）

# 强制真实模型差分（缺 sample 金样或模型即失败；复核整改 3b / B5）：
HUX_REQUIRE_SAMPLE=1 cargo test -p hux-scheme-tiger --test ngram_differential

# 金样 sha 表 / 内部头部 / 参照文件溯源（复核整改第 4 批 M2；CI 两个作业各跑一次；
# 表与 pin 声明在 regenerate.md）
python3 tools/checks/verify_golden_shas.py [--reference external/tiger-sentense-rime]

# 基准（ngram，真实模型 + 本地抽样金样）
cargo run --release -q --example ngram_bench -- <model.bin> <transcript.tsv>
lua tools/probes/bench_ngram.lua --reference "$REF" --model <model.bin> --transcript <transcript.tsv>
```

## Lua 版本

- 一般作业用 CI 系统 Lua；`golden-lua-latest` 用 Arch 容器当前 Lua；生成器摘要 JSON 记录实际版本。

## 规则

- **金样不得因本仓有意的行为差异而重生成**：金样记录的是**上游参照行为**。若判定上游某行为为缺陷而有意偏离
  （或按用户决定强化语义），只能在差分测试中把受影响用例连同**本仓逐步期望值**登记进 `DEVIATIONS`
  （可证伪：断言「期望 ≠ 金样」的步集合恰等于「实测 ≠ 金样」的步集合，且非空），
  金样字节保持原样；**待上游修复后删除登记、恢复无条件逐位比对**。现有偏离项见 [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)。
