<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：差分金样

金样由参照实现（[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 的 Lua 核心）生成，Rust 侧逐位重放比对
（`crates/hux-scheme/tiger/tests/*_differential.rs` 与 `crates/hux-core/tests/*_differential.rs`）。

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
| `key.tsv.gz` | 键名/键事件金样（librime 探针）：`name`/`repr`/`parse`/`modifier` | 5132 行 |
| `key_sequence.tsv.gz` | 键序列金样（真 librime 探针；主干 pin）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 57 例 / 242 步（含空码自动上屏、编辑/导航键、标点表、大写字母直接提交；`punct_menu_equal`/`punct_menu_minus` 记录**上游行为**「菜单可见的 ASCII 标点先确认组合再交标点表」——其中 `punct_menu_equal` 因上游缺陷（翻页绑定被标点分支遮蔽）被本仓**有意偏离**、在差分测试中登记跳过，`punct_menu_minus` 仍逐位一致，见 `docs/refactor.md` §8「有意偏离上游」） |
| `key_sequence/` | 键序列夹具（合成码表 + `symbols.yaml`＝参照 pin 同文件；探针与 Rust 重放共用；发布默认见 `data/symbols.yaml`） | 2 文件 |
| `sound_to_char_shape.tsv.gz` | 音反查金样（真 librime 探针；反查分支尖端 pin，已含主干）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 31 例 / 164 步（裸前缀标点候选、缩写/全拼剪枝、多音节词、Page 键翻页、导航/退格/Escape/上屏、数字直选与分号惰性、撇号保留；`nav-page-equal`/`nav-page-minus`/`nav-page-zho` 记录**上游行为**「`=`/`-` 被标点分支遮蔽」，因上游缺陷被本仓**有意偏离**、在差分测试中登记跳过，其余（含 `nav-page-keys`）逐位一致，见 `docs/refactor.md` §8「有意偏离上游」） |
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

## 重新生成

> **先决条件（实测踩坑）**：夹具类生成器（`gen_ngram_/lexicon_/decode_/learning_/lexical_golden.lua`）
> 通过 `package.path = <reference>/lua/?.lua` 读参照仓库的**工作区**，**不认 pin**。
> 因此生成前必须把参照检出租到目标 pin（CI 的做法是
> `git -C "$REF" fetch --depth 1 origin "$REFERENCE_COMMIT" && git -C "$REF" checkout --detach FETCH_HEAD`），
> 否则会静默读到工作区里更靠后的核心版本，产出与本次追平无关的金样差异。
> 探针脚本（`gen_key_sequence_golden.sh` / `gen_sound_to_char_shape_golden.sh`）用 `git show PIN:`
> 或 `git worktree add --detach PIN` 自建临时工作区，本身是 pin 精确的；后者另有护栏：
> HEAD 不是 `PIN`（例如有人重新引入本地合并）或工作区不干净时**显式失败**。

```sh
# 参照仓库：https://github.com/lvyww/tiger-sentense-rime
# 本地检出（金样生成用；命令均在仓库根目录执行；外部检出统一放 external/，已 gitignore）
git clone https://github.com/lvyww/tiger-sentense-rime external/tiger-sentense-rime
REF=external/tiger-sentense-rime

# ngram fixture（入库）
lua tools/generators/gen_ngram_golden.lua --reference "$REF" \
  --model goldens/ngram_fixture.bin --out /tmp/ngram_fixture.tsv --mode fixture
gzip -9 -n -c /tmp/ngram_fixture.tsv > goldens/ngram_fixture.tsv.gz

# ngram 真实模型抽样（本地）
lua tools/generators/gen_ngram_golden.lua --reference "$REF" \
  --model ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  --out goldens/local/ngram_sample.tsv --mode sample
gzip -9 -n -c goldens/local/ngram_sample.tsv > goldens/local/ngram_sample.tsv.gz

# lexicon（入库）
lua tools/generators/gen_lexicon_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/lexicon.tsv --mode present
lua tools/generators/gen_lexicon_golden.lua --reference "$REF" \
  --data /tmp/no-such-dir --out /tmp/lexicon_missing.tsv --mode missing
lua tools/generators/gen_lexicon_golden.lua --reference "$REF" \
  --data "goldens/lexicon_variants" --out /tmp/lexicon_variants.tsv --mode present
lua tools/generators/gen_lexicon_golden.lua --reference "$REF" \
  --data "goldens/lexicon_codes_only" --out /tmp/lexicon_codes_only.tsv --mode present
gzip -9 -n -c /tmp/lexicon.tsv > goldens/lexicon.tsv.gz
gzip -9 -n -c /tmp/lexicon_missing.tsv > goldens/lexicon_missing.tsv.gz
gzip -9 -n -c /tmp/lexicon_variants.tsv > goldens/lexicon_variants.tsv.gz
gzip -9 -n -c /tmp/lexicon_codes_only.tsv > goldens/lexicon_codes_only.tsv.gz

# decode（入库；模型版对 fixture 抽样）
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode.tsv
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_model.tsv --every 7
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_rank_first.tsv --every 7 --duplicate 0
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode_evidence.tsv --early-commit 1 --required 1
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_evidence_model.tsv --every 7 --early-commit 1 --required 1
gzip -9 -n -c /tmp/decode.tsv > goldens/decode.tsv.gz
gzip -9 -n -c /tmp/decode_model.tsv > goldens/decode_model.tsv.gz
gzip -9 -n -c /tmp/decode_rank_first.tsv > goldens/decode_rank_first.tsv.gz
gzip -9 -n -c /tmp/decode_evidence.tsv > goldens/decode_evidence.tsv.gz
gzip -9 -n -c /tmp/decode_evidence_model.tsv > goldens/decode_evidence_model.tsv.gz

# decode + 学习（入库；`--learning 1` 的学习索引由真实候选纠错事件 + 一条成对融合偏好构成）
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode_learning.tsv --learning 1
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_learning_model.tsv --every 7 --learning 1
gzip -9 -n -c /tmp/decode_learning.tsv > goldens/decode_learning.tsv.gz
gzip -9 -n -c /tmp/decode_learning_model.tsv > goldens/decode_learning_model.tsv.gz

# learning（入库；生成器对 corpora/chains/diffcases 按名排序迭代，输出与 Lua 进程哈希序无关）
lua tools/generators/gen_learning_golden.lua --reference "$REF" --out /tmp/learning.tsv
gzip -9 -n -c /tmp/learning.tsv > goldens/learning.tsv.gz

# key（入库；只需要系统 librime；pin 版 key_table.cc 单文件下载即可，无需克隆）
mkdir -p external/librime/src/rime
curl -fsSL -o external/librime/src/rime/key_table.cc \
  https://raw.githubusercontent.com/rime/librime/33e78140250125871856cdc5b42ddc6a5fcd3cd4/src/rime/key_table.cc
bash tools/generators/gen_key_golden.sh external/librime    # 脚本校验文件 sha 与 key_table.rs 头部一致

# key_sequence（入库；需要系统 librime + librime-lua，构建 pin 版隔离环境）
bash tools/generators/gen_key_sequence_golden.sh
# 探索新用例时可用 CASES 指向临时用例文件（输出默认仍写入入库文件，建议显式给输出路径）：
# CASES=/tmp/explore.txt bash tools/generators/gen_key_sequence_golden.sh /tmp/explore.tsv.gz

# sound_to_char_shape（入库；同上；夹具索引由生成器顺带重建）
#   默认 PIN = 反查分支尖端 92a0b54（已含主干 pin，故不再做本地合并；PIN 可覆盖）
bash tools/generators/gen_sound_to_char_shape_golden.sh

# lexical（入库；需要参照的词先验模块与 data/ 位图；CI 已接入）
lua tools/generators/gen_lexical_golden.lua --reference "$REF" --model data/tiger_sentence.lexical.bin --out /tmp/lexical.tsv
gzip -9 -n -c /tmp/lexical.tsv > goldens/lexical.tsv.gz
```

## 校验

```sh
cargo test --workspace        # 全部差分（本地 sample 缺失自动跳过）

# 基准（ngram，真实模型 + 本地抽样金样）
cargo run --release -q --example ngram_bench -- <model.bin> <transcript.tsv>
lua tools/probes/bench_ngram.lua --reference "$REF" --model <model.bin> --transcript <transcript.tsv>
```

## Lua 版本

- 一般作业用 CI 系统 Lua；`golden-lua-latest` 用 Arch 容器当前 Lua；生成器摘要 JSON 记录实际版本。

## 来源与校验和

- **主干 pin**：[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) @
  `abad411750f79cfca750985fa266689b5d9b865f`（main 尖端，`fix(rime): preserve punctuation learning and default to full-m5`）。
  **由 Lua 核心生成的 15 份夹具 / decode / learning / lexical 金样，以及键序列探针金样 `key_sequence.tsv.gz`，
  都取自该 pin。**
- **反查分支 pin**：[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) @
  `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`（`feat/reverse-lookup` 尖端：`4ff37c4` 数字选择器提交反查候选、
  `92a0b54` 撇号音节分隔）。该 pin 是**主干 pin 的后代**（`abad411` 在其祖先链上），因此音反查探针金样
  `sound_to_char_shape.tsv.gz` 单独取自它，**不再需要「分支 + 主干本地合并」**：
  `tools/generators/gen_sound_to_char_shape_golden.sh` 已简化为单 `PIN` + 护栏
  （HEAD 必须等于 `PIN` 且工作区干净，否则显式失败）。
- **键名表**：librime `src/rime/key_table.cc`（sha256 `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`，固定提交 `33e78140`）；
  `key_table.rs` 由 `tools/generators/gen_key_table.py` 生成（CI 单文件下载源码后重生成比对）；`key.tsv.gz` 由系统 librime 1.17.0 探针生成，**CI 不重生成**。
- **键序列 / 音反查**：`key_sequence.tsv.gz`、`sound_to_char_shape.tsv.gz` 由 `tools/probes/rime_sequence_probe.cpp` 驱动
  **真 librime + librime-lua** 与对应 pin 版 Lua 核心生成（探针头部记录参照提交与源文件 sha256），**CI 不重生成**；
  夹具入库并与 Rust 重放共用，其中音反查夹具索引由 `tools/generators/gen_pinyin_index.py` 生成（CI 重生成比对）。
  两者都依赖探针所用 librime/librime-lua 版本；音反查金样的撇号用例尤其如此——`92a0b54` 的
  「按 `speller/delimiter` 切分音节」依赖上游 librime 的 delimiter 修复
  （[rime/librime#1233](https://github.com/rime/librime/pull/1233)），
  本机 librime 1.17.0 未含该修复，故输入撇号后反查段**无候选**（金样如实记录该行为）。
  真实索引（`data/tiger_sentence.pinyin.bin.gz`，sha256 `18a0931a…`）由同一生成器产出，本地复验可重新生成并比对：
  `python3 tools/generators/gen_pinyin_index.py --source external/tiger-sentense-rime/PY_c.dict.yaml --out /tmp/pinyin.bin.gz && cmp /tmp/pinyin.bin.gz data/tiger_sentence.pinyin.bin.gz`
  （参照检出须含 `898579f` 的 `PY_c.dict.yaml`）。
- **金样不得因本仓有意的行为差异而重生成**：金样记录的是**上游参照行为**。若判定上游某行为为缺陷而有意偏离，
  只能在差分测试中把受影响用例登记进 `DEVIATED_CASES` 跳过（并断言「实际跳过集合恰等于登记集合」），
  金样字节保持原样；**待上游修复后删除登记、恢复无条件逐位比对**。现有偏离项见 `docs/refactor.md` §8「有意偏离上游」。
- **词先验**：`lexical.tsv.gz` 由 `tools/generators/gen_lexical_golden.lua` 以参照 main（词先验模块自 `35a10b9` 起提供）与
  入库位图生成（CC BY 4.0，见 [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)）；
  **已在 CI 中再生成比对**。
- 参照仓库文件（生成时；`lua/`、`tools/` 均为参照仓库路径；两 pin 相同的文件只列一行）：

| 文件 | 来源 pin | sha256 |
|---|---|---|
| `lua/tiger_sentence.lua`（主干金样） | 主干 `abad411` | `b77a747597a140e6fec315d8bc78344b8d8d3bdc007132bc7c53a9c6a3f22dd3` |
| `lua/tiger_sentence.lua`（音反查金样） | 反查 `92a0b54` | `f33cee28f78a612d77570297a6949732f46eeb3c4011b7fe760f43c3b3120b89` |
| `lua/tiger_sentence_learning.lua` | 两 pin 相同 | `0f685ae57fb4e70662492b7a3e56b91b5e8e9592cc64d881db181c9bf7acd9c6` |
| `lua/tiger_sentence_ngram.lua` | 两 pin 相同 | `fd7b2337d5215f51ffea092c76f07951a8e2172087e823d8a4b1641f11d8bf4e` |
| `lua/tiger_sentence_cache.lua` | 两 pin 相同 | `8ebd209588fb62d0bf888e752b95d8588ecbcdef2af40f8b009865fc3c41da7c` |
| `lua/tiger_sentence_lexical.lua` | 两 pin 相同 | `d49f45f0ee0033fd2466269d967b4784f508da220ec62215806e227ea590fe8d` |
| `tools/model_fixture.lua` | 主干 `abad411` | `ed5c771ee29835c20b46476635809ed37d70ad0c79d14df0ae13233f5da7d45a` |

- 数据夹具（`lexicon/`，取自参照仓库同名文件）：

| 文件 | sha256 |
|---|---|
| `tiger_sentence.codes.txt` | `1d3e9b0ce0e4a603be3f220c71acecad846f020e87a52723ecb3814f6b53ac0e` |
| `tiger_sentence.char_ranks.txt` | `bd64e4bf333b2096a9a61fd5ece868e37912057bd1a812d75b2d5ccb4c994dcf` |
| `tiger_sentence.full_code_whitelist.txt` | `05d257457898146262f7dbf264103c70a8cf2ee92d188b770ad13232b293f566` |
| `tiger_sentence.supplement.txt` | `f229832bc92f89d87e4b1d29984aec53e627cedb23dda5074ad03cbcabdf0900`（**本地改动**：仅注释中方案名「虎整句」→「虎句」，与上游 pin 的 `538f7d60…` 不同；见 `../data/README.md`） |
| `key_sequence/symbols.yaml` | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `sound_to_char_shape/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `sound_to_char_shape/PY_c.dict.yaml`（夹具） | `96e8b34adebf5ea478a1cbce2c9ee8f333c264690abfffadd2d31642a30360ee` |
| `sound_to_char_shape/tiger_sentence.codes.txt`（夹具） | `4e2b7596db232e12ad997613155e067354652288270b55852d3fd2f16ab18709` |
| `sound_to_char_shape/tiger_sentence.pinyin.bin`（生成物） | `29e16c6aa40654ca829197996584912efc9f76f61bcd9370c1341999b69f3e4d` |

- `lexicon_variants/` 与 `lexicon_codes_only/` 为人工构造的解析边界数据（无上游来源）。

- 已入库金样 sha256：

| 文件 | sha256 |
|---|---|
| `ngram_fixture.bin` | `b50a12fc5292fabbd4841fd61dd7f85cfa6ae9987d1f74aa752287aa6f86862f` |
| `ngram_fixture.tsv.gz` | `905dfac57fafd2a4eb55d18cc0d23b9ac5b363aecee55cc610f755bd8ccfb9ee` |
| `lexicon.tsv.gz` | `28b410dc42a5a17bfb93138843139d3946a6decc3d3ea5d867f70740f5242135` |
| `lexicon_missing.tsv.gz` | `f5b8256deeb41b4403ca26074deec659807c4313ffe5cf727987b78be15c7a26` |
| `lexicon_variants.tsv.gz` | `05923b1433f00bf2e9fbb6270e6b28e1f4d1cca6a93507c74dc80b48fde69ef5` |
| `lexicon_codes_only.tsv.gz` | `3cd72cca880754ecd3744a26ecc5b70d8b5575ab268654937326bb4925b3805e` |
| `decode.tsv.gz` | `79c6316038d9b18feb42214e8fef20853d9f3a1ae963b792064367d9ef72757d` |
| `decode_model.tsv.gz` | `a81bc37713b293deaab17ee4a8c51dfe2c244ff1e8729f7bb21514f98b895dc2` |
| `decode_rank_first.tsv.gz` | `4359b9276805e99433e9787ac35abf37ae3eab7d11aa49bce2549dedb1799fc6` |
| `decode_evidence.tsv.gz` | `bd857dade791a040d6a0d4ffc9a6e302524cd6fd7095f2aea967193ec280d7af` |
| `decode_evidence_model.tsv.gz` | `fb45281a2c1eae7e3d9910e346adcbfb9c6af60a452636ce7347bd6c2d8987d5` |
| `learning.tsv.gz` | `d70c69b29a37761bcadd87dd2d3bd3a9679eaaadf658827036cdb1c104186d4d` |
| `decode_learning.tsv.gz` | `1e46d3fbb4788e36b92d92ac931b343ad2d7f394e9611e1a04f36bf034f90cdd` |
| `decode_learning_model.tsv.gz` | `13797b96097bcc133f862528350f4765d2c021f88c9f8e7ad56d5e6e02de881c` |
| `key.tsv.gz` | `e939a077cd0825f7b454a4af300ed50fb6a2f2609c71583525d44f2f8fb3fd33` |
| `key_sequence.tsv.gz` | `486897c902c23dffe1bc7bdc457f81ca49c2d1689ed3e9d921f94acd457367fe` |
| `sound_to_char_shape.tsv.gz` | `e9d48698bf73807a37933b7c2324afbc27fffe7b0492f0dd2787116ec06a7545` |
| `lexical.tsv.gz` | `5b559b2504e21c69b4f702678a96d2947abfe7d7c26adcd2b25c3d4de761e0c3` |

CI 以同一参照提交重生成全部 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
`key.tsv.gz`、`key_sequence.tsv.gz`、`sound_to_char_shape.tsv.gz` 依赖具体 librime/librime-lua 版本，**CI 不重生成**
（改由 CI 按上表校验其 sha256）。
