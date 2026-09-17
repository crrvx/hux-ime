# goldens：差分金样

金样由参照实现（[`crrvx/tiger-sentense-rime`](https://github.com/crrvx/tiger-sentense-rime) 的 Lua 核心）生成，Rust 侧逐位重放比对
（`crates/hux-core/tests/*_differential.rs`）。

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
| `key_sequence.tsv.gz` | 键序列金样（真 librime 探针）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 55 例 / 236 步（含空码自动上屏、编辑/导航键、标点表、大写字母直接提交） |
| `key_sequence/` | 键序列夹具（合成码表 + `symbols.yaml`＝参照 pin 同文件；探针与 Rust 重放共用；发布默认见 `data/symbols.yaml`） | 2 文件 |
| `pinyin_lookup.tsv.gz` | 音查虎金样（真 librime 探针，pin `898579f`）：逐步 `consumed`/输入/光标/提交/候选/注释/高亮 | 24 例 / 127 步（裸前缀标点候选、缩写/全拼剪枝、多音节词、翻页 `=`/`-`/Page 键、导航/退格/Escape/上屏） |
| `pinyin_lookup/` | 音查虎夹具（小 `PY_c.dict.yaml` + 合成码表 + `symbols.yaml` + `gen_pinyin_index.py` 生成的 `tiger_sentence.pinyin.bin`；探针与 Rust 重放共用） | 4 文件 + 生成物 |
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

# key_sequence / pinyin_lookup（真 librime 探针）
case <case> <options>
step <case> <index> <repr> <consumed 0/1> <input> <caret> <commit> <preedit>
     <page> <highlight> <candidate_count> <candidates> <comments>
```

## 重新生成

```sh
# 参照仓库：https://github.com/crrvx/tiger-sentense-rime
# 本地检出（金样生成用；命令均在仓库根目录执行；外部检出统一放 external/，已 gitignore）
git clone https://github.com/crrvx/tiger-sentense-rime external/tiger-sentense-rime
REF=external/tiger-sentense-rime

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
  --data "goldens/lexicon" --out /tmp/lexicon.tsv --mode present
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data /tmp/no-such-dir --out /tmp/lexicon_missing.tsv --mode missing
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data "goldens/lexicon_variants" --out /tmp/lexicon_variants.tsv --mode present
lua tools/gen_lexicon_golden.lua --reference "$REF" \
  --data "goldens/lexicon_codes_only" --out /tmp/lexicon_codes_only.tsv --mode present
gzip -9 -n -c /tmp/lexicon.tsv > goldens/lexicon.tsv.gz
gzip -9 -n -c /tmp/lexicon_missing.tsv > goldens/lexicon_missing.tsv.gz
gzip -9 -n -c /tmp/lexicon_variants.tsv > goldens/lexicon_variants.tsv.gz
gzip -9 -n -c /tmp/lexicon_codes_only.tsv > goldens/lexicon_codes_only.tsv.gz

# decode（入库；模型版对 fixture 抽样）
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode.tsv
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_model.tsv --every 7
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_rank_first.tsv --every 7 --duplicate 0
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode_evidence.tsv --early-commit 1 --required 1
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_evidence_model.tsv --every 7 --early-commit 1 --required 1
gzip -9 -n -c /tmp/decode.tsv > goldens/decode.tsv.gz
gzip -9 -n -c /tmp/decode_model.tsv > goldens/decode_model.tsv.gz
gzip -9 -n -c /tmp/decode_rank_first.tsv > goldens/decode_rank_first.tsv.gz
gzip -9 -n -c /tmp/decode_evidence.tsv > goldens/decode_evidence.tsv.gz
gzip -9 -n -c /tmp/decode_evidence_model.tsv > goldens/decode_evidence_model.tsv.gz

# decode + 学习（入库）
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode_learning.tsv --learning 1
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --model "goldens/ngram_fixture.bin" \
  --lexical "data/tiger_sentence.lexical.bin" \
  --out /tmp/decode_learning_model.tsv --every 7 --learning 1
gzip -9 -n -c /tmp/decode_learning.tsv > goldens/decode_learning.tsv.gz
gzip -9 -n -c /tmp/decode_learning_model.tsv > goldens/decode_learning_model.tsv.gz

# learning（入库；生成器对 corpora/chains/diffcases 按名排序迭代，输出与 Lua 进程哈希序无关）
lua tools/gen_learning_golden.lua --reference "$REF" --out /tmp/learning.tsv
gzip -9 -n -c /tmp/learning.tsv > goldens/learning.tsv.gz

# key（入库；需要 librime 源码头文件与系统 librime；源码检出放 external/，pin 与键表来源一致）
git clone https://github.com/rime/librime external/librime
git -C external/librime checkout 33e78140250125871856cdc5b42ddc6a5fcd3cd4
bash tools/gen_key_golden.sh external/librime

# key_sequence（入库；需要系统 librime + librime-lua，构建 pin 版隔离环境）
bash tools/gen_key_sequence_golden.sh
# 探索新用例时可用 CASES 指向临时用例文件（输出默认仍写入入库文件，建议显式给输出路径）：
# CASES=/tmp/explore.txt bash tools/gen_key_sequence_golden.sh /tmp/explore.tsv.gz

# pinyin_lookup（入库；同上；夹具索引由生成器顺带重建）
bash tools/gen_pinyin_lookup_golden.sh

# lexical（入库；需要参照的词先验模块与 data/ 位图；CI 已接入）
lua tools/gen_lexical_golden.lua --reference "$REF" --model data/tiger_sentence.lexical.bin --out /tmp/lexical.tsv
gzip -9 -n -c /tmp/lexical.tsv > goldens/lexical.tsv.gz
```

## 校验

```sh
cargo test -p hux-core        # 全部差分（本地 sample 缺失自动跳过）

# 基准（ngram，真实模型 + 本地抽样金样）
cargo run --release -q --example ngram_bench -- <model.bin> <transcript.tsv>
lua tools/bench_ngram.lua --reference "$REF" --model <model.bin> --transcript <transcript.tsv>
```

## Lua 版本

- 一般作业用 CI 系统 Lua；`golden-lua-latest` 用 Arch 容器当前 Lua；生成器摘要 JSON 记录实际版本。

## 来源与校验和

- **主干**：[`crrvx/tiger-sentense-rime`](https://github.com/crrvx/tiger-sentense-rime) @ `8b615235c17c858e1eca8f1a41fbc74e202f8bbe`（main）。
- **音查虎**：`feat/reverse-lookup` @ `898579f833df53f1dec5639d56e685751a8a7f71` + 上述 main **本地合并**
  （上游未合并该分支；`tools/gen_pinyin_lookup_golden.sh` 自建临时 worktree 合并，`PIN`/`BASE` 可覆盖）。
- **键名表**：librime `src/rime/key_table.cc`（sha256 `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`，固定提交 `33e78140`）；
  `key_table.rs` 由 `tools/gen_key_table.py` 生成（CI 重生成比对）；`key.tsv.gz` 由系统 librime 1.17.0 探针生成，**CI 不重生成**。
- **键序列 / 音查虎**：`key_sequence.tsv.gz`、`pinyin_lookup.tsv.gz` 由 `tools/rime_sequence_probe.cpp` 驱动
  **真 librime + librime-lua** 与 pin 版 Lua 核心生成（探针头部记录参照提交与源文件 sha256），**CI 不重生成**；
  夹具入库并与 Rust 重放共用，其中音查虎夹具索引由 `tools/gen_pinyin_index.py` 生成（CI 重生成比对）。
  真实索引（`data/tiger_sentence.pinyin.bin.gz`）的校验和与来源见
  [`../docs/PINYIN_INDEX_MANIFEST.json`](../docs/PINYIN_INDEX_MANIFEST.json)，本地复验：
  `python3 tools/gen_pinyin_index.py --source external/tiger-sentense-rime/PY_c.dict.yaml --out data/tiger_sentence.pinyin.bin.gz --check --manifest docs/PINYIN_INDEX_MANIFEST.json`。
- **词先验**：`lexical.tsv.gz` 由 `tools/gen_lexical_golden.lua` 以参照 main（词先验模块自 `35a10b9` 起提供）与
  入库位图生成（CC BY 4.0，见 [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)）；
  **已在 CI 中再生成比对**。
- 参照仓库文件（生成时；`lua/`、`tools/` 均为参照仓库路径）：

| 文件 | sha256 |
|---|---|
| `lua/tiger_sentence.lua` | `dfcc687ea28d1174a99c37aaf1d3de7d0dc69332a8aaf4bfac3079506efa2047` |
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
| `tiger_sentence.supplement.txt` | `f229832bc92f89d87e4b1d29984aec53e627cedb23dda5074ad03cbcabdf0900` |
| `key_sequence/symbols.yaml` | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `pinyin_lookup/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `pinyin_lookup/PY_c.dict.yaml`（夹具） | `96e8b34adebf5ea478a1cbce2c9ee8f333c264690abfffadd2d31642a30360ee` |
| `pinyin_lookup/tiger_sentence.codes.txt`（夹具） | `4e2b7596db232e12ad997613155e067354652288270b55852d3fd2f16ab18709` |
| `pinyin_lookup/tiger_sentence.pinyin.bin`（生成物） | `29e16c6aa40654ca829197996584912efc9f76f61bcd9370c1341999b69f3e4d` |

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
| `learning.tsv.gz` | `d8eff6b6b67cf8803f9965e71d171ec9fb88f3e7e3bc61368c11e19ac70358cf` |
| `decode_learning.tsv.gz` | `41a9894d233c32348e42164d4d29fc698c3037741c141ac0b58404094c9e9354` |
| `decode_learning_model.tsv.gz` | `8a64e6e3d28b101a57075b03233e62c4b03e8c4a8d2a979399a91a00e2d8e806` |
| `key.tsv.gz` | `e939a077cd0825f7b454a4af300ed50fb6a2f2609c71583525d44f2f8fb3fd33` |
| `key_sequence.tsv.gz` | `d67cf237617de2615907c04e43c99bda165a61fd0820013db447c99384c83721` |
| `pinyin_lookup.tsv.gz` | `6fcea93e7cbc12952d7d0b4a7333a4e824a4a22a08f4a21df37b45fef5a219c4` |
| `lexical.tsv.gz` | `5b559b2504e21c69b4f702678a96d2947abfe7d7c26adcd2b25c3d4de761e0c3` |

CI 以同一参照提交重生成全部 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
`key.tsv.gz` 与 `key_sequence.tsv.gz` 依赖具体 librime/librime-lua 版本，**CI 不重生成**。
