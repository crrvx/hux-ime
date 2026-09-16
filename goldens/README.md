# goldens：差分金样

金样由参照实现（`tiger-sentense-rime` 的 Lua 核心）生成，Rust 侧逐位重放比对
（`crates/tigerclaw-core/tests/*_differential.rs`）。

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
| `decode_evidence.tsv.gz` | 早提交证据金样（无模型；含 `has_complete_candidate` 的 `complete` 用例） | 11002 行 |
| `decode_evidence_model.tsv.gz` | 早提交证据金样（fixture 模型，抽样；同上） | 1824 行 |
| `learning.tsv.gz` | 学习金样：`hash`/`score`/`prefix`/`confirmed`/`reward`/`diff`/日志编码 | 10164 行 |
| `decode_learning.tsv.gz` | 解码接入学习（无模型） | 1987 行 |
| `decode_learning_model.tsv.gz` | 解码接入学习（fixture 模型，抽样） | 358 行 |
| `key.tsv.gz` | 键名/键事件金样（librime 探针）：`name`/`repr`/`parse`/`modifier` | 5132 行 |
| `key_sequence.tsv.gz` | 键序列金样（真 librime 探针，2c）：逐步 `consumed`/输入/光标/提交/候选/高亮 | 44 例 / 241 步（含空码自动上屏、编辑/导航键、ascii Shift 切换、大写字母 DirectCommit） |
| `key_sequence/` | 键序列夹具码表（探针与 Rust 重放共用） | 1 文件 |
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
result <hex text> <hex segmented> <bits score> <bits confidence_score> <max_rank> <edge_count> <bits supplement_score> <bits learning_score>
# decode + 早提交证据（--early-commit 1）
evidence <hex proposal> <bits proposal_share> nit= mit= nlc= trunc= prefixes= raws=
prefix <hex text> <raw_length> <bits share> <bits boundary_share> <closed> <chars>
rawlen <hex text> <raw_length>
# decode + 学习接入（--learning 1）
learningsetup <now> <hex mode> <n>   # 后接 n 条 levent
levent <time> <hex mode> <hex code> <hex text> <hex ctx>

# learning（纯计算；见生成器头部注释的完整字段表）
hash <hex text> <value>
corpus / event / index / confirmed / codes / score / prefix / trim
chain / node / reward / diffcase / diffpath / diff / diffevent
journalrecords / journalrecord / journalevents / journalevent

# key（librime 探针）
name <keyval> <name|->
repr <keyval> <modifier> <repr>
parse <repr> <ok|bad> <keycode> <modifier> <repr>
modifier <index> <name|->
```

## 重新生成

```sh
REF=/path/to/tiger-sentense-rime

# ngram fixture（入库）
lua tools/gen_ngram_golden.lua --reference "$REF" \
  --model goldens/ngram_fixture.bin --out /tmp/ngram_fixture.tsv --mode fixture
gzip -9 -n -c /tmp/ngram_fixture.tsv > goldens/ngram_fixture.tsv.gz

# ngram 真实模型抽样（本地）
lua tools/gen_ngram_golden.lua --reference "$REF" \
  --model ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  --out goldens/local/ngram_sample.tsv --mode sample
gzip -9 -n -c goldens/local/ngram_sample.tsv > goldens/local/ngram_sample.tsv.gz

# lexicon（入库）
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --out /tmp/lexicon.tsv --mode present
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data /tmp/no-such-dir --out /tmp/lexicon_missing.tsv --mode missing
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon_variants" --out /tmp/lexicon_variants.tsv --mode present
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon_codes_only" --out /tmp/lexicon_codes_only.tsv --mode present
gzip -9 -n -c /tmp/lexicon.tsv > goldens/lexicon.tsv.gz
gzip -9 -n -c /tmp/lexicon_missing.tsv > goldens/lexicon_missing.tsv.gz
gzip -9 -n -c /tmp/lexicon_variants.tsv > goldens/lexicon_variants.tsv.gz
gzip -9 -n -c /tmp/lexicon_codes_only.tsv > goldens/lexicon_codes_only.tsv.gz

# decode（入库；模型版对 fixture 抽样）
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --out /tmp/decode.tsv
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --lexical "$PWD/data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_model.tsv --every 7
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --lexical "$PWD/data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_rank_first.tsv --every 7 --duplicate 0
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --out /tmp/decode_evidence.tsv --early-commit 1 --required 1
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --lexical "$PWD/data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_evidence_model.tsv --every 7 --early-commit 1 --required 1
gzip -9 -n -c /tmp/decode.tsv > goldens/decode.tsv.gz
gzip -9 -n -c /tmp/decode_model.tsv > goldens/decode_model.tsv.gz
gzip -9 -n -c /tmp/decode_rank_first.tsv > goldens/decode_rank_first.tsv.gz
gzip -9 -n -c /tmp/decode_evidence.tsv > goldens/decode_evidence.tsv.gz
gzip -9 -n -c /tmp/decode_evidence_model.tsv > goldens/decode_evidence_model.tsv.gz

# decode + 学习（入库）
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --out /tmp/decode_learning.tsv --learning 1
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --lexical "$PWD/data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_learning_model.tsv --every 7 --learning 1
gzip -9 -n -c /tmp/decode_learning.tsv > goldens/decode_learning.tsv.gz
gzip -9 -n -c /tmp/decode_learning_model.tsv > goldens/decode_learning_model.tsv.gz

# learning（入库）
lua tools/gen_learning_golden.lua --reference "$REF" --out /tmp/learning.tsv
gzip -9 -n -c /tmp/learning.tsv > goldens/learning.tsv.gz

# key（入库；需要 librime 源码头文件与系统 librime）
bash tools/gen_key_golden.sh /path/to/librime

# key_sequence（入库；需要系统 librime + librime-lua，构建 pin 版隔离环境）
bash tools/gen_key_sequence_golden.sh
# 探索新用例时可用 CASES 指向临时用例文件（输出默认仍写入入库文件，建议显式给输出路径）：
# CASES=/tmp/explore.txt bash tools/gen_key_sequence_golden.sh /tmp/explore.tsv.gz

# lexical（入库；需要参照的词先验模块与 data/ 位图；CI 已接入）
lua tools/gen_lexical_golden.lua --reference "$REF" --model data/tiger_sentence.lexical.bin --out /tmp/lexical.tsv
gzip -9 -n -c /tmp/lexical.tsv > goldens/lexical.tsv.gz
```

## 校验

```sh
cargo test -p tigerclaw-core        # 全部差分（本地 sample 缺失自动跳过）

# 基准（ngram，真实模型 + 本地抽样金样）
cargo run --release -q --example ngram_bench -- <model.bin> <transcript.tsv>
lua tools/bench_ngram.lua --reference "$REF" --model <model.bin> --transcript <transcript.tsv>
```

## Lua 版本

- 一般作业使用 CI 系统提供的 Lua；
- `golden-lua-latest` 作业使用 Arch 容器当前的 Lua；
- 生成器摘要 JSON 记录实际运行的 Lua 版本。

## 来源与校验和

- 参照实现：`crrvx/tiger-sentense-rime` @ `35a10b93c96af7b008fc9a05d01a8381018dc3d3`（main）
- 键名表来源：librime `src/rime/key_table.cc`（sha256 `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`，librime 1.17.0 固定提交 `33e78140`）；
  `key_table.rs` 由 `tools/gen_key_table.py` 生成，CI 以同提交重新生成并比对；`key.tsv.gz` 由系统 librime 1.17.0 探针（`tools/key_probe.cpp`）生成，
  因探针依赖具体 librime 版本，**CI 不重生成该金样**（仅按 Rust 侧重放校验 + 键表生成比对）。
- 键序列金样：`key_sequence.tsv.gz` 由 `tools/rime_sequence_probe.cpp` 在隔离环境中驱动**真 librime + librime-lua** 与 pin 版 Lua 核心生成
  （探针头部记录参照提交、`tiger_sentence.lua` sha256 与 librime 版本）；同样**不在 CI 重生成**。数据夹具 `key_sequence/` 入库并与 Rust 重放共用。
- 词先验金样：`lexical.tsv.gz` 由 `tools/gen_lexical_golden.lua` 以参照 main `35a10b9`（词先验模块随该提交进入 main）
  与入库位图 `data/tiger_sentence.lexical.bin` 生成（CC BY 4.0，见 `docs/LEXICAL_PRIOR_ATTRIBUTION.md`）；
  语料取自参照码表与确定性采样，重放不依赖外部词表与网络；**已在 CI 中再生成比对**。
- 参照仓库文件（生成时；`lua/`、`tools/` 均为参照仓库路径）：

| 文件 | sha256 |
|---|---|
| `lua/tiger_sentence.lua` | `fe11e07da98bd3223136a89e283d80e7c01b90c14c0ccba6cbcfd283927778b7` |
| `lua/tiger_sentence_learning.lua` | `335e530bb42b8fa2c432b900a0e5ff9d7509e74a8674d099456083088b36f85e` |
| `lua/tiger_sentence_ngram.lua` | `a3d59e09fbff3b09b0ac79ef66b7560210b5503c2af38eb5615069d6465cb361` |
| `lua/tiger_sentence_cache.lua` | `8ebd209588fb62d0bf888e752b95d8588ecbcdef2af40f8b009865fc3c41da7c` |
| `tools/model_fixture.lua` | `ed5c771ee29835c20b46476635809ed37d70ad0c79d14df0ae13233f5da7d45a` |

- 数据夹具（`lexicon/`，取自参照仓库同名文件）：

| 文件 | sha256 |
|---|---|
| `tiger_sentence.codes.txt` | `1d3e9b0ce0e4a603be3f220c71acecad846f020e87a52723ecb3814f6b53ac0e` |
| `tiger_sentence.char_ranks.txt` | `bd64e4bf333b2096a9a61fd5ece868e37912057bd1a812d75b2d5ccb4c994dcf` |
| `tiger_sentence.full_code_whitelist.txt` | `05d257457898146262f7dbf264103c70a8cf2ee92d188b770ad13232b293f566` |
| `tiger_sentence.supplement.txt` | `538f7d60ae378235628a86e7ef20d24396453488fde950a88e52fdb6f558a5ac` |

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
| `decode.tsv.gz` | `0fb6182ec3379a1eac865870cfa8b214ecb0eab044fc6ec3f4bf07783a6c92f6` |
| `decode_model.tsv.gz` | `9fcdf60ce2eaa0a07e0a7749b04a8abfe586e259feb1b2002bd14d1cf08e8c35` |
| `decode_rank_first.tsv.gz` | `6df164942f6de48c48921118e32dff9297d4fdc381524baebeca6562a98c0ae0` |
| `decode_evidence.tsv.gz` | `357e782cb2e1528e76e9e066fbc2c0dacaf7b77ea8b21f71659769cad4d938ec` |
| `decode_evidence_model.tsv.gz` | `96289e3254228c9dec63806db2ab738da2d3cb11bd0adad2e0eb672210a3e766` |
| `learning.tsv.gz` | `58392f2e5aec6ab5f87c116b366747b0d0c746fc0bab5d13361821b8bbeead32` |
| `decode_learning.tsv.gz` | `41a9894d233c32348e42164d4d29fc698c3037741c141ac0b58404094c9e9354` |
| `decode_learning_model.tsv.gz` | `8a64e6e3d28b101a57075b03233e62c4b03e8c4a8d2a979399a91a00e2d8e806` |
| `key.tsv.gz` | `e939a077cd0825f7b454a4af300ed50fb6a2f2609c71583525d44f2f8fb3fd33` |
| `key_sequence.tsv.gz` | `734a6b1e7358390abfd6a610542f3d667b406c948906e6c07717b8cd3422dec5` |
| `lexical.tsv.gz` | `4b56476d28bd1a070fe72352561828264faba85e01df1ea47d908cbaeff28842` |

CI 以同一参照提交重生成全部 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
`key.tsv.gz` 与 `key_sequence.tsv.gz` 依赖具体 librime/librime-lua 版本，**CI 不重生成**。
