<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：金样清单、transcript 格式与规则

- 金样生成：参照实现（Lua 核心）
  - [`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime)
- 金样比对：Rust 侧逐位重放
  - `crates/hux-scheme/tiger/tests/*_differential.rs`
  - `crates/hux-core/tests/*_differential.rs`

本页＝清单、transcript 格式与校验入口；另两册：

- 重新生成见 [`REGENERATE.md`](REGENERATE.md)：
  - 覆盖生成命令、先决条件、参照 pin 检出与踩坑。
- 来源与校验和见 [`PROVENANCE.md`](PROVENANCE.md)：
  - 覆盖 pin、sha256 表、内部头部核对。

## 内容

| 文件 | 用途 | 规模 |
|---|---|---|
| `ngram_fixture.bin` | ngram 确定性小模型 | 17,480 B |
| `ngram_fixture.tsv.gz` | ngram 金样 | 29617 行 |
| `lexicon/` | 码表数据夹具 | 4 文件 |
| `lexicon.tsv.gz` | lexicon 金样 | 18357 行 |
| `lexicon_missing.tsv.gz` | 数据缺失路径金样 | 6 行 |
| `lexicon_variants/` + `lexicon_variants.tsv.gz` | 解析边界数据 | 27 条 |
| `lexicon_codes_only/` + `lexicon_codes_only.tsv.gz` | 仅码表 | 13 条 |
| `decode.tsv.gz` | decode 金样（无模型） | 1980 行 |
| `decode_model.tsv.gz` | decode 金样（fixture 模型，抽样） | 351 行 |
| `decode_rank_first.tsv.gz` | decode 金样（抽样） | 330 行 |
| `decode_evidence.tsv.gz` | 早提交证据金样（无模型） | 12241 行 |
| `decode_evidence_model.tsv.gz` | 早提交证据金样（抽样） | 2861 行 |
| `learning.tsv.gz` | 学习金样 | 10289 行 |
| `decode_learning.tsv.gz` | 解码接入学习（无模型） | 1988 行 |
| `decode_learning_model.tsv.gz` | 解码接入学习（抽样） | 359 行 |
| `decode_learning_evidence.tsv.gz` | 早提交证据 + 学习接入 | 12257 行 |
| `key.tsv.gz` | 键名/键事件金样（librime 探针） | 5136 行 |
| `key_sequence.tsv.gz` | 键序列金样（真 librime 探针） | 68 例 / 285 步 |
| `key_sequence/` | 键序列夹具 | 2 文件 |
| `key_sequence_tab.tsv.gz` | Tab 锁路径金样 | 8 例 / 44 步 |
| `key_sequence_tab/` | Tab 金样夹具 | 3 文件 |
| `sound_to_char_shape.tsv.gz` | 音反查金样<br>（真 librime 探针） | 31 例 / 164 步 |
| `sound_to_char_shape/` | 音反查夹具 | 4 文件（含生成物） |
| `lexical.tsv.gz` | 词先验金样 | 753 行 |
| `local/`（不入库） | 真实模型抽样金样 | 62,777 条 |

各文件细则：

- `ngram_fixture.bin`
  - 生成：参照仓库 `tools/model_fixture.lua`。
- `ngram_fixture.tsv.gz`
  - 字段：`logp` / `obs` / `status` / `cfg` / `trim`。
- `lexicon/`
  - 构成：codes / char_ranks / full_code_whitelist / supplement。
- `lexicon.tsv.gz`
  - 字段：`status` / `lengths` / `probe` / `limit` / `supp`。
- `lexicon_variants/` + `lexicon_variants.tsv.gz`
  - 覆盖：BOM/CRLF/大写码/重复/非法行/白名单豁免/高频过滤。
- `lexicon_codes_only/` + `lexicon_codes_only.tsv.gz`
  - 口径：无字频/白名单/补充。
- `decode.tsv.gz`
  - 字段：`decode` / `result`。
- `decode_rank_first.tsv.gz`
  - 口径：fixture 模型 + 关闭单字重码，抽样。
- `decode_evidence.tsv.gz`
  - 用例：含 `has_complete_candidate` 的 `complete`。
- `decode_evidence_model.tsv.gz`
  - 口径：fixture 模型，抽样。
  - 用例：同上（`has_complete_candidate` 的 `complete`）。
- `learning.tsv.gz`
  - 字段一：`hash` / `score` / `prefix` / `confirmed` / `reward`。
  - 字段二：成熟度 / `diff` / 融合偏好 / 人工纠错等级 / 日志编码。
- `decode_learning.tsv.gz`
  - 口径：无模型；含一条成对融合偏好。
- `decode_learning_model.tsv.gz`
  - 口径：fixture 模型，抽样；同上。
- `decode_learning_evidence.tsv.gz`
  - 口径：三开关并用，无模型。
  - 开关与交互见下。
- `key.tsv.gz`
  - 记录数：5132 条 + 4 行头部。
  - 字段：`name` / `repr` / `parse` / `modifier`。
- `key_sequence.tsv.gz`
  - 探针：真 librime；主干 pin。
  - 逐步字段：`consumed` / 输入 / 光标 / 提交 / 候选 / 注释 / 高亮。
  - 覆盖与偏离登记见下。
- `key_sequence/`
  - 构成：合成码表 + `symbols.yaml`＝参照 pin 同文件。
  - 用途：探针与 Rust 重放共用。
  - 发布默认见 `data/symbols.yaml`。
- `key_sequence_tab.tsv.gz`
  - 探针：真 librime；主干 pin。
  - 夹具 `tab_learning: true` ⇒ 参照学习库就绪。
  - 走 `learned.store.db` 的 Tab 基线捕获分支。
  - 逐步字段同 `key_sequence`。
  - 用例名见下。
- `key_sequence_tab/`
  - `symbols.yaml` 与合成码表与 `key_sequence/` **逐字节相同**。
  - 另含 `tiger_sentence.custom.yaml`。
  - 它把 `tab_learning: true` 这一**条件本身**入库。
- `sound_to_char_shape.tsv.gz`
  - 探针：真 librime；反查分支尖端 pin，已含主干。
  - 逐步字段：`consumed` / 输入 / 光标 / 提交 / 候选 / 注释 / 高亮。
  - 覆盖与偏离登记见下。
- `sound_to_char_shape/`
  - 构成：本仓自建小 `PY_c.dict.yaml` + 合成码表 + `symbols.yaml`。
  - 另含 `gen_pinyin_index.py` 生成的 `tiger_sentence.pinyin.bin`。
  - 用途：探针与 Rust 重放共用。
- `lexical.tsv.gz`
  - 口径：TCSLEX01 读取 / Bloom / 打分。
  - 语料：真实位图 + 码表语料。
- `local/`（不入库）
  - 模型：224 MB。

要点（上表「见下」的细节）：

- `key_sequence.tsv.gz`
  - 覆盖点：
    - 空码自动上屏 / 编辑 / 导航键 / 标点表。
    - 大写字母直接提交 / `Return` 修饰键变体（`Ctrl(+Shift)+Return`）。
    - 标点表 caret 语义 / selector 首页上翻。
    - 数字直选 / 撇号分段。
  - 偏离登记：
    - `punct_menu_equal` / `punct_menu_minus` / `nav_page_home_minus`。
    - `digit_menu_select`。
    - `apostrophe_digit_page` / `apostrophe_semicolon_page`。
    - 这六条记录的是**上游行为**，由差分测试按期望值登记。
    - 理由 / 代价 / 回归见 ①②③：
      - [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)
- `key_sequence_tab.tsv.gz`
  - 用例：
    - `tab_lock` / `tab_confirm_space` / `tab_confirm_buffer`。
    - `tab_buffer_space` / `nav_tab_binder` / `tab_lock_left`。
    - `tab_lock_backspace` / `tab_lock_up_down`。
- `sound_to_char_shape.tsv.gz`
  - 覆盖点：
    - 裸前缀标点候选 / 缩写 / 全拼剪枝。
    - 多音节词 / Page 键翻页。
    - 导航 / 退格 / Escape / 上屏。
    - 数字直选与分号惰性 / 撇号保留。
  - 偏离登记：
    - `nav-page-equal` / `nav-page-minus` / `nav-page-zho`。
    - 这三条记录的是**上游行为**，由差分测试按期望值登记跳过。
    - 其余（含 `nav-page-keys`）逐位一致。
    - 见 ①：
      - [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)
- `decode_learning_evidence.tsv.gz`
  - 开关与交互：
    - 三开关并用：`--early-commit 1 --required 1 --learning 1`（无模型）。
    - 学习 × 证据抑制的交互：
      - `learning=1 && truncated=1` 的截断池。
      - 与 `share` / `base_share` 双权重。

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
result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank>
       <edge_count> <bits supplement_score> <bits learning_score>
       <bits early_commit_confidence_score>
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

# 强制真实模型差分（缺 sample 金样或模型即失败）：
HUX_REQUIRE_SAMPLE=1 cargo test -p hux-scheme-tiger --test ngram_differential

# 金样 sha 表 / 内部头部 / 参照文件溯源（无需网络 / 参照检出）：
python3 tools/checks/verify_golden_shas.py
# 追加校验参照仓库文件（lua/*、tools/*）与各自 pin 的 sha256；需要完整检出（勿用 --depth 1）：
python3 tools/checks/verify_golden_shas.py --reference _external/tiger-sentense-rime

# 基准（ngram，真实模型 + 本地抽样金样）
cargo run --release -q -p hux-scheme-tiger --example ngram_bench -- <model.bin> <transcript.tsv>
lua tools/probes/bench_ngram.lua --reference "$REF" --model <model.bin> \
  --transcript <transcript.tsv>
```

## Lua 版本

- 一般作业用 CI 系统 Lua。
- `golden-lua-latest` 用 Arch 容器当前 Lua。
- 生成器摘要 JSON 记录实际版本。
- 重新生成后用 `tools/checks/verify_golden_shas.py` 校验产物。
- 产物 sha256 须与 [`PROVENANCE.md`](PROVENANCE.md) 的对照表一致。

## 规则

- **金样不得因本仓有意的行为差异而重生成**：
  - 金样记录的是**上游参照行为**。
  - 偏离只能在差分测试里连同**本仓逐步期望值**登记进 `DEVIATIONS`。
  - 金样字节保持原样。
  - 完整政策（可证伪断言、代价与回归做法）见：
    - [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)
