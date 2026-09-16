# goldens：差分金样

金样由参照实现（`tiger-sentense-rime` 的 Lua 5.4 核心）生成，Rust 侧逐位重放比对
（`crates/tigerclaw-core/tests/ngram_differential.rs`）。

## 内容

| 文件 | 用途 | 说明 |
|---|---|---|
| `ngram_fixture.bin` | 确定性小模型 | 由参照仓库 `tools/model_fixture.lua` 生成（17,480 B） |
| `ngram_fixture.tsv.gz` | fixture 金样 transcript | 29,617 条记录；含独立 float32 oracle 自检 |
| `local/`（不入库） | 真实模型抽样金样 | `sentence-ngram-mobile.bin`（224 MB）抽样 62,777 条 |

transcript 格式（tab 分隔，`#` 注释）：

```
bytes  <file_size>
logp   <hex a> <hex b> <hex c> <0x hi|lo bits>   # f64 位模式，hi/lo 两个 u32
obs    <hex a> <hex b> <0|1>
status <k=v>...                                   # cache_status 规范快照
cfg    <page> <context> <bigram> <index>          # 执行 configure_cache
trim                                              # 执行 trim_caches
close                                             # 执行 close
```

空串参数编码为 `-`；字符串为 UTF-8 字节的小写十六进制。

## 重新生成

```sh
# fixture（入库）
lua5.4 tools/gen_ngram_golden.lua \
  --reference /path/to/tiger-sentense-rime \
  --model goldens/ngram_fixture.bin \
  --out /tmp/ngram_fixture.tsv --mode fixture
gzip -9 -n -c /tmp/ngram_fixture.tsv > goldens/ngram_fixture.tsv.gz

# 真实模型抽样（本地）
lua5.4 tools/gen_ngram_golden.lua \
  --reference /path/to/tiger-sentense-rime \
  --model ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  --out goldens/local/ngram_sample.tsv --mode sample
gzip -9 -n -c goldens/local/ngram_sample.tsv > goldens/local/ngram_sample.tsv.gz
```

## 校验

```sh
cargo test -p tigerclaw-core                       # fixture + 本地 sample（缺失则跳过）
cargo run --release -q --example ngram_bench -- <model.bin> <transcript.tsv>
lua5.4 tools/bench_ngram.lua --reference /path/to/tiger-sentense-rime \
  --model <model.bin> --transcript <transcript.tsv>
```

## 来源与校验和

- 参照实现：`crrvx/tiger-sentense-rime` @ `f3b3049819b513ba756bbe6c6b6872759c9dc2a9`
- 生成时参照文件 sha256：

| 文件 | sha256 |
|---|---|
| `lua/tiger_sentence.lua` | `989a9207eb06755d547b14a752707c147753274147a33e939d765429f2e0e848` |
| `lua/tiger_sentence_learning.lua` | `335e530bb42b8fa2c432b900a0e5ff9d7509e74a8674d099456083088b36f85e` |
| `lua/tiger_sentence_ngram.lua` | `a3d59e09fbff3b09b0ac79ef66b7560210b5503c2af38eb5615069d6465cb361` |
| `lua/tiger_sentence_cache.lua` | `8ebd209588fb62d0bf888e752b95d8588ecbcdef2af40f8b009865fc3c41da7c` |
| `tools/model_fixture.lua` | `ed5c771ee29835c20b46476635809ed37d70ad0c79d14df0ae13233f5da7d45a` |

- 已入库金样 sha256：

| 文件 | sha256 |
|---|---|
| `ngram_fixture.bin` | `b50a12fc5292fabbd4841fd61dd7f85cfa6ae9987d1f74aa752287aa6f86862f` |
| `ngram_fixture.tsv.gz` | `905dfac57fafd2a4eb55d18cc0d23b9ac5b363aecee55cc610f755bd8ccfb9ee` |

CI 以同一参照提交重生成 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
