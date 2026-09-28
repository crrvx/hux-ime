<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：重新生成

- 本页＝生成命令与实操踩坑
- 清单 / 格式见 [`README.md`](README.md)；来源 pin 与 sha256 表见 [`PROVENANCE.md`](PROVENANCE.md)

## 一键入口

- 重跑全部 Lua 金样并逐字节比对，与 CI 的 `golden` / `golden-lua-latest` 两作业共用
- 位置参数依次为参考检出 / 金样目录 / 临时目录：

`bash tools/generators/regen_goldens.sh [<参考检出>] [<金样目录>] [<临时目录>]`

## 先决条件（实测踩坑）

- 夹具类生成器**不认 pin**：走 `package.path = <reference>/lua/?.lua` 读参照仓库**工作区**
  - 这些生成器是 `gen_ngram_/lexicon_/decode_/learning_/lexical_golden.lua`
  - 故命令块第一件事就是检出主干 pin；否则会静默读到更靠后的核心版本、产出与目标 pin 无关的差异
- 参照检出**只读**时（发行版打包目录 / 只读挂载）在仓库之外另放可写克隆，该克隆要取全两个 pin：

    ```sh
    git clone https://github.com/lvyww/tiger-sentense-rime "$HOME/ref/tiger-sentense-rime"  # 或 cp -r 已有检出
    RW="$HOME/ref/tiger-sentense-rime"
    git -C "$RW" fetch origin abad411750f79cfca750985fa266689b5d9b865f
    git -C "$RW" fetch origin 92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c
    ```

- **不要用 `--depth 1` / `--shallow`**：浅克隆只带个别 tip
  - 此时 `git show <pin>:` 与 `git worktree add --detach <pin>` 都会失败
- CI 的 `golden` 作业逐个 pin 取、不合并：先 `git init`，再逐个 pin \
  执行 `git fetch --depth 1 origin <sha>`，然后 `checkout --detach FETCH_HEAD`
  - 故 CI 可用浅克隆
- 探针脚本自建临时工作区（`git show PIN:` 或 `git worktree add --detach PIN`），与上面的 \
  checkout 无关；这类工作区 pin 精确，不需要先 checkout：
  - `gen_key_sequence_golden.sh`
  - `gen_key_sequence_tab_golden.sh`
  - `gen_sound_to_char_shape_golden.sh`
- 三者另有护栏：HEAD 不是 `PIN` 或工作区不干净时**显式失败**
- 三者输出都**只认位置参数**：设 `OUT=` 会被忽略，直接写回入库金样
  - 要写到别处就传第一个位置参数：`bash <脚本> /tmp/x.tsv.gz`
- `gen_key_golden.sh` 只依赖系统 librime 与 pin 版 `key_table.cc`，与参照检出无关

## 逐条命令

```sh
# 参照仓库：https://github.com/lvyww/tiger-sentense-rime
# 命令均在仓库根目录执行；外部检出统一放 external/（已 gitignore）。
# `set -e`：中途失败即停，避免把不完整 TSV 压进入库金样。
set -euo pipefail
git clone https://github.com/lvyww/tiger-sentense-rime _external/tiger-sentense-rime
REF=_external/tiger-sentense-rime

# ① 检出主干 pin（**必须**：以下 5 段夹具类生成器读参照工作区，不认 pin）
git -C "$REF" checkout --detach abad411750f79cfca750985fa266689b5d9b865f
# ② 生成前自检：HEAD 必须是该 pin（检出失败/被切走即停）
test "$(git -C "$REF" rev-parse HEAD)" = abad411750f79cfca750985fa266689b5d9b865f

# ③ ngram fixture（入库）
lua tools/generators/gen_ngram_golden.lua --reference "$REF" \
  --model goldens/ngram_fixture.bin --out /tmp/ngram_fixture.tsv --mode fixture
gzip -9 -n -c /tmp/ngram_fixture.tsv > goldens/ngram_fixture.tsv.gz

# ngram 真实模型抽样（本地）
lua tools/generators/gen_ngram_golden.lua --reference "$REF" \
  --model ~/.local/share/fcitx5/hux/models/sentence-ngram-mobile.bin \
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

# decode + 学习 + 早提交证据（入库；两个开关本来就可在生成器里并用，此处补齐**组合覆盖**）
lua tools/generators/gen_decode_golden.lua --reference "$REF" \
  --data "goldens/lexicon" --out /tmp/decode_learning_evidence.tsv \
  --early-commit 1 --required 1 --learning 1
gzip -9 -n -c /tmp/decode_learning_evidence.tsv > goldens/decode_learning_evidence.tsv.gz

# learning（入库；生成器对 corpora/chains/diffcases 按名排序迭代，输出与 Lua 进程哈希序无关）
lua tools/generators/gen_learning_golden.lua --reference "$REF" --out /tmp/learning.tsv
gzip -9 -n -c /tmp/learning.tsv > goldens/learning.tsv.gz

# key（入库；只需要系统 librime；pin 版 key_table.cc 单文件下载即可，无需参照检出）
mkdir -p external/librime/src/rime
curl -fsSL -o external/librime/src/rime/key_table.cc \
  https://raw.githubusercontent.com/rime/librime/33e78140250125871856cdc5b42ddc6a5fcd3cd4/src/rime/key_table.cc
# 脚本校验文件 sha 与 `key_table.rs` 头部一致
bash tools/generators/gen_key_golden.sh external/librime

# key_sequence（入库；需要系统 librime + librime-lua；脚本自建 `PIN=abad411` 隔离工作区，
# 与上面的检出状态无关——不必先 checkout）
bash tools/generators/gen_key_sequence_golden.sh
# 探索新用例时可用 CASES 指向临时用例文件（输出默认仍写入入库文件，建议显式给输出路径）：
# CASES=/tmp/explore.txt bash tools/generators/gen_key_sequence_golden.sh /tmp/explore.tsv.gz

# key_sequence_tab（入库；Tab 锁路径：夹具 `tab_learning: true` ⇒ 参照学习库就绪；
#   与 key_sequence 同一探针/pin，脚本自建隔离工作区，不需要上面的 checkout）
bash tools/generators/gen_key_sequence_tab_golden.sh

# sound_to_char_shape（入库；同上；夹具索引由生成器顺带重建）
#   默认 PIN = 反查分支尖端 92a0b54（已含主干 pin，故不再做本地合并；PIN 可覆盖）；
#   同样自建隔离工作区，不需要上面的 checkout
bash tools/generators/gen_sound_to_char_shape_golden.sh

# lexical（入库；需要参照的词先验模块与 data/ 位图；**要主线 pin 的工作区**；CI 已接入）
lua tools/generators/gen_lexical_golden.lua --reference "$REF" \
  --model data/tiger_sentence.lexical.bin --out /tmp/lexical.tsv
gzip -9 -n -c /tmp/lexical.tsv > goldens/lexical.tsv.gz
```
