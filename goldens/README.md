# goldens：差分金样

金样由参照实现（`tiger-sentense-rime` 的 Lua 核心）生成，Rust 侧逐位重放比对
（`crates/tigerclaw-core/tests/*_differential.rs`）。

## 内容

| 文件 | 用途 | 规模 |
|---|---|---|
| `ngram_fixture.bin` | ngram 确定性小模型（`tools/model_fixture.lua` 生成） | 17,480 B |
| `ngram_fixture.tsv.gz` | ngram 金样：`logp`/`obs`/`status`/`cfg`/`trim` | 29,617 条 |
| `lexicon/` | 码表数据夹具（codes / char_ranks / full_code_whitelist / supplement） | 4 文件 |
| `lexicon.tsv.gz` | lexicon 金样：`status`/`lengths`/`probe`/`limit`/`supp` | 18,357 条 |
| `lexicon_missing.tsv.gz` | 数据缺失路径金样 | 5 条 |
| `lexicon_variants/` + `lexicon_variants.tsv.gz` | 解析边界数据（BOM/CRLF/大写码/重复/非法行/白名单豁免/高频过滤） | 27 条 |
| `lexicon_codes_only/` + `lexicon_codes_only.tsv.gz` | 仅码表（无字频/白名单/补充） | 13 条 |
| `decode.tsv.gz` | decode 金样（无模型）：`decode`/`result` | 1,980 条 |
| `decode_model.tsv.gz` | decode 金样（fixture 模型，抽样） | 278 条 |
| `decode_rank_first.tsv.gz` | decode 金样（fixture 模型 + 关闭单字重码，抽样） | 330 条 |
| `decode_evidence.tsv.gz` | 早提交证据金样（无模型） | 4,998 条 |
| `decode_evidence_model.tsv.gz` | 早提交证据金样（fixture 模型，抽样） | 840 条 |
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
  --out /tmp/decode_model.tsv --every 7
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --out /tmp/decode_rank_first.tsv --every 7 --duplicate 0
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --out /tmp/decode_evidence.tsv --early-commit 1 --required 1
lua tools/gen_decode_golden.lua --reference "$REF" \
  --data "$PWD/goldens/lexicon" --model "$PWD/goldens/ngram_fixture.bin" \
  --out /tmp/decode_evidence_model.tsv --every 7 --early-commit 1 --required 1
gzip -9 -n -c /tmp/decode.tsv > goldens/decode.tsv.gz
gzip -9 -n -c /tmp/decode_model.tsv > goldens/decode_model.tsv.gz
gzip -9 -n -c /tmp/decode_rank_first.tsv > goldens/decode_rank_first.tsv.gz
gzip -9 -n -c /tmp/decode_evidence.tsv > goldens/decode_evidence.tsv.gz
gzip -9 -n -c /tmp/decode_evidence_model.tsv > goldens/decode_evidence_model.tsv.gz
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

- 参照实现：`crrvx/tiger-sentense-rime` @ `f3b3049819b513ba756bbe6c6b6872759c9dc2a9`
- 参照 Lua 文件（生成时）：

| 文件 | sha256 |
|---|---|
| `lua/tiger_sentence.lua` | `989a9207eb06755d547b14a752707c147753274147a33e939d765429f2e0e848` |
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
| `decode.tsv.gz` | `997a68e68077da7af63a155a01900e94fbb11b71cb9c064cd3c31eb55415c090` |
| `decode_model.tsv.gz` | `a543ae32f83b88791b3dbb99f748da8e5add1d26590b096d561eecf532bbcfbb` |
| `decode_rank_first.tsv.gz` | `ea08e0c2bcc6da841b2b52af189cde82dc4eb054c6dc6552d7167d99517c841e` |
| `decode_evidence.tsv.gz` | `8d1952082c7cf937d91943786224f8cd55ee9cd92b2bf3892b7073b7861898fd` |
| `decode_evidence_model.tsv.gz` | `d75c3b093121fed6114f88dcf5ebe10a01c862c42ae31f3d5927d32889388181` |

CI 以同一参照提交重生成全部 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
