<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：重新生成与校验和

> 金样**是什么 / 内容表 / transcript 格式 / 校验入口 / 规则**见 [`README.md`](README.md)。
> 本文只放**重新生成命令全表**与**来源、校验和（sha 表）**。

## 重新生成

> **先决条件（实测踩坑）**：夹具类生成器（`gen_ngram_/lexicon_/decode_/learning_/lexical_golden.lua`）
> 通过 `package.path = <reference>/lua/?.lua` 读参照仓库的**工作区**，**不认 pin**。
> 因此生成前必须把参照检出租到目标 pin——下面的命令块**第一件事就是
> `git -C "$REF" checkout --detach 9f742d275c2bd50c7c664be1c258a7b8429e83a1`（主干 pin）**，
> 否则会静默读到工作区里更靠后的核心版本，产出与本次追平无关的金样差异。
> 参照检出**只读**（无法 `checkout`，例如发行版打包目录 / 只读挂载）时的替代做法：
> 在**仓库之外**另放一个可写克隆，再对它 `fetch` + `checkout`（主干 pin 与反查 pin 都取）：
>
> ```sh
> git clone https://github.com/lvyww/tiger-sentense-rime "$HOME/ref/tiger-sentense-rime"  # 或 cp -r 已有检出
> RW="$HOME/ref/tiger-sentense-rime"
> git -C "$RW" fetch origin 9f742d275c2bd50c7c664be1c258a7b8429e83a1
> git -C "$RW" fetch origin 92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c
> ```
>
> **不要用 `--depth 1` / `--shallow`**：浅克隆会让需要「分支 pin + 主干 pin 本地合并」的历史操作
> 被判为**无关历史**（`fatal: refusing to merge unrelated histories`），也会让 `git show <pin>:` 失败。
> （CI 的 `golden` 作业是另一次性的只读检出，用 `git init` + `git fetch --depth 1 origin <sha>` +
> `checkout --detach FETCH_HEAD` 逐个 pin 取；那里不合并，故可用浅克隆。）
> 探针脚本（`gen_key_sequence_golden.sh` / `gen_sound_to_char_shape_golden.sh`）用 `git show PIN:`
> 或 `git worktree add --detach PIN` 自建临时工作区，本身是 pin 精确的、**不需要**上面的 checkout；
> 后者另有护栏：HEAD 不是 `PIN`（例如有人重新引入本地合并）或工作区不干净时**显式失败**。
> `gen_key_golden.sh` 只依赖系统 librime 与 pin 版 `key_table.cc` 单文件，与参照检出无关。

```sh
# 参照仓库：https://github.com/lvyww/tiger-sentense-rime
# 命令均在仓库根目录执行；外部检出统一放 external/（已 gitignore）。
# `set -e`：中途失败即停，避免把不完整 TSV 压进入库金样。
set -euo pipefail
git clone https://github.com/lvyww/tiger-sentense-rime _external/tiger-sentense-rime
REF=_external/tiger-sentense-rime

# ① 检出主干 pin（**必须**：以下 5 段夹具类生成器读参照工作区，不认 pin）
git -C "$REF" checkout --detach 9f742d275c2bd50c7c664be1c258a7b8429e83a1
# ② 生成前自检：HEAD 必须是该 pin（检出失败/被切走即停）
test "$(git -C "$REF" rev-parse HEAD)" = 9f742d275c2bd50c7c664be1c258a7b8429e83a1

# ③ ngram fixture（入库）
lua tools/generators/gen_ngram_golden.lua --reference "$REF" \
  --model goldens/ngram_fixture.bin --out /tmp/ngram_fixture.tsv --mode fixture
gzip -9 -n -c /tmp/ngram_fixture.tsv > goldens/ngram_fixture.tsv.gz

# 五阶 fixture（入库；上游 builder 编译 + 固定 ARPA，脚本产出后自检 sha；依赖 g++/python3）
bash tools/generators/gen_fivegram_fixture.sh

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
bash tools/generators/gen_key_golden.sh external/librime    # 脚本校验文件 sha 与 key_table.rs 头部一致

# key_sequence（入库；需要系统 librime + librime-lua；脚本自建 `PIN=9f742d2` 隔离工作区，
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
lua tools/generators/gen_lexical_golden.lua --reference "$REF" --model data/tiger_sentence.lexical.bin --out /tmp/lexical.tsv
gzip -9 -n -c /tmp/lexical.tsv > goldens/lexical.tsv.gz
```

生成后（或在 CI / 本地复核时）用校验器确认产物与本文档的校验和表一致：

```sh
# 表 ↔ 文件、三份探针金样的内部头部 ↔ 声明的 pin（无需网络 / 参照检出）
python3 tools/checks/verify_golden_shas.py
# 追加校验参照仓库文件（lua/*、tools/*）与各自 pin 的 sha256；需要完整检出（勿用 --depth 1）
python3 tools/checks/verify_golden_shas.py --reference _external/tiger-sentense-rime
```

## 来源与校验和

- **主干 pin**：[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) @
  `9f742d275c2bd50c7c664be1c258a7b8429e83a1`（main 尖端，`feat: add pure Lua TCSKNM03 fivegram search`；该提交只对提供 `step` 的新模型生效，
  对 TCSKNM02 金样路径逐行无差异）。
  **由 Lua 核心生成的 16 份夹具 / decode / learning / lexical 金样，以及两份键序列探针金样
  （`key_sequence.tsv.gz`、`key_sequence_tab.tsv.gz`），都取自该 pin。**
- **反查分支 pin**：[`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) @
  `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`（`feat/reverse-lookup` 尖端：`4ff37c4` 数字选择器提交反查候选、
  `92a0b54` 撇号音节分隔）。该 pin 是 `abad411`（当时的主干 pin）的**后代**；主干其后新增的 TCSKNM03 提交不在其中（不影响本金样的
  TCSKNM02 路径）。因此音反查探针金样
  `sound_to_char_shape.tsv.gz` 单独取自它，**不再需要「分支 + 主干本地合并」**：
  `tools/generators/gen_sound_to_char_shape_golden.sh` 已简化为单 `PIN` + 护栏
  （HEAD 必须等于 `PIN` 且工作区干净，否则显式失败）。
- **键名表**：librime `src/rime/key_table.cc`（sha256 `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`，固定提交
  `33e78140250125871856cdc5b42ddc6a5fcd3cd4`）；
  `key_table.rs` 由 `tools/generators/gen_key_table.py` 生成（CI 单文件下载源码后重生成比对）；`key.tsv.gz` 由系统 librime 1.17.0 探针生成，**CI 不重生成**
  （其内部头部记录本行的 pin 与 sha256，由 `tools/checks/verify_golden_shas.py` 核对）。
- **键序列 / 音反查 / Tab 锁**：`key_sequence.tsv.gz`、`key_sequence_tab.tsv.gz`、`sound_to_char_shape.tsv.gz` 由 `tools/probes/rime_sequence_probe.cpp` 驱动
  **真 librime + librime-lua** 与对应 pin 版 Lua 核心生成（探针头部记录参照提交与源文件 sha256），**CI 不重生成**；
  夹具入库并与 Rust 重放共用，其中音反查夹具索引由 `tools/generators/gen_pinyin_index.py` 生成（CI 重生成比对）。
  两者都依赖探针所用 librime/librime-lua 版本；音反查金样的撇号用例尤其如此——`92a0b54` 的
  「按 `speller/delimiter` 切分音节」依赖上游 librime 的 delimiter 修复
  （[rime/librime#1233](https://github.com/rime/librime/pull/1233)），
  本机 librime 1.17.0 未含该修复，故输入撇号后反查段**无候选**（金样如实记录该行为）。
  真实索引（`data/tiger_sentence.pinyin.bin.gz`，sha256 `18a0931a…`）由同一生成器产出，本地复验可重新生成并比对：
  `python3 tools/generators/gen_pinyin_index.py --source _external/tiger-sentense-rime/PY_c.dict.yaml --out /tmp/pinyin.bin.gz && cmp /tmp/pinyin.bin.gz data/tiger_sentence.pinyin.bin.gz`
  （参照检出须含 `898579f` 的 `PY_c.dict.yaml`）。
- **词先验**：`lexical.tsv.gz` 由 `tools/generators/gen_lexical_golden.lua` 以参照 main（词先验模块自 `35a10b9` 起提供）与
  入库位图生成（CC BY 4.0，见 [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)）；
  **已在 CI 中再生成比对**。
- 参照仓库文件（生成时；`lua/`、`tools/` 均为参照仓库路径；两 pin 相同的文件只列一行）：

| 文件 | 来源 pin | sha256 |
|---|---|---|
| `lua/tiger_sentence.lua`（主干金样） | 主干 `9f742d2` | `702df0c49e6402cd216e1fad8b26353e80fd01d3f3923366768a5b3b058df735` |
| `lua/tiger_sentence.lua`（音反查金样） | 反查 `92a0b54` | `f33cee28f78a612d77570297a6949732f46eeb3c4011b7fe760f43c3b3120b89` |
| `lua/tiger_sentence_learning.lua` | 两 pin 相同 | `0f685ae57fb4e70662492b7a3e56b91b5e8e9592cc64d881db181c9bf7acd9c6` |
| `lua/tiger_sentence_ngram.lua`（主干金样） | 主干 `9f742d2` | `f05a1beb0a6347aaf3c436c41a1ea91e8b64845271bfc9db8139044ffe066926` |
| `lua/tiger_sentence_ngram.lua`（音反查金样） | 反查 `92a0b54` | `fd7b2337d5215f51ffea092c76f07951a8e2172087e823d8a4b1641f11d8bf4e` |
| `lua/tiger_sentence_fivegram.lua`（主干新增） | 主干 `9f742d2` | `0514c61727c037f336194c012daf7ed32c034cad3197555417e96bc0fef2cce6` |
| `lua/tiger_sentence_cache.lua` | 两 pin 相同 | `8ebd209588fb62d0bf888e752b95d8588ecbcdef2af40f8b009865fc3c41da7c` |
| `lua/tiger_sentence_lexical.lua` | 两 pin 相同 | `d49f45f0ee0033fd2466269d967b4784f508da220ec62215806e227ea590fe8d` |
| `tools/model_fixture.lua` | 主干 `9f742d2` | `ed5c771ee29835c20b46476635809ed37d70ad0c79d14df0ae13233f5da7d45a` |

- 数据夹具（`lexicon/`，取自参照仓库同名文件）：

| 文件 | sha256 |
|---|---|
| `tiger_sentence.codes.txt` | `1d3e9b0ce0e4a603be3f220c71acecad846f020e87a52723ecb3814f6b53ac0e` |
| `tiger_sentence.char_ranks.txt` | `bd64e4bf333b2096a9a61fd5ece868e37912057bd1a812d75b2d5ccb4c994dcf` |
| `tiger_sentence.full_code_whitelist.txt` | `05d257457898146262f7dbf264103c70a8cf2ee92d188b770ad13232b293f566` |
| `tiger_sentence.supplement.txt` | `f229832bc92f89d87e4b1d29984aec53e627cedb23dda5074ad03cbcabdf0900`（**本地改动**：仅注释中方案名「虎整句」→「虎句」，与上游 pin 的 `538f7d60…` 不同；见 `../data/README.md`） |
| `key_sequence/symbols.yaml` | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `key_sequence_tab/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `key_sequence_tab/tiger_sentence.custom.yaml`（夹具条件：`tab_learning: true`） | `c610834a73205b5d584ad752c31b557df9547225e8924a9eb1fccc99cbd42ca7` |
| `sound_to_char_shape/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `sound_to_char_shape/PY_c.dict.yaml`（夹具） | `96e8b34adebf5ea478a1cbce2c9ee8f333c264690abfffadd2d31642a30360ee` |
| `sound_to_char_shape/tiger_sentence.codes.txt`（夹具） | `4e2b7596db232e12ad997613155e067354652288270b55852d3fd2f16ab18709` |
| `sound_to_char_shape/tiger_sentence.pinyin.bin`（生成物） | `29e16c6aa40654ca829197996584912efc9f76f61bcd9370c1341999b69f3e4d` |

- `lexicon_variants/` 与 `lexicon_codes_only/` 为人工构造的解析边界数据（无上游来源）。

- 已入库金样 sha256：

| 文件 | sha256 |
|---|---|
| `fivegram_fixture_paged.bin` | `357f3adc8cb57e11ed788ee59fa55f8a89ae4c4fd6eca62e9b9a44564b026132` |
| `fivegram_fixture.bin` | `0c3581b2b84baa7e782de25bcbe513e26fabb8835e9b2f370de88a614f8ebda9` |
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
| `decode_learning_evidence.tsv.gz` | `86dec38d94f05e57de281e254262a40aef86176965d739afee6eb6b644464398` |
| `key.tsv.gz` | `7fae4983ab69e36ebd2e5cac267df81e9bc325731deaefcaa22873aeca8660c0` |
| `key_sequence.tsv.gz` | `68994c55aeee8a8a0a74c3e1683b6a605b1dfed6d34fc05adde39e3da1a05b24` |
| `key_sequence_tab.tsv.gz` | `dd2dcd5549ff6f1ef5f532bd0fffe4f109359a8a7fdb5ce098d6c800232ae7b1` |
| `sound_to_char_shape.tsv.gz` | `e9d48698bf73807a37933b7c2324afbc27fffe7b0492f0dd2787116ec06a7545` |
| `lexical.tsv.gz` | `5b559b2504e21c69b4f702678a96d2947abfe7d7c26adcd2b25c3d4de761e0c3` |

- **内部头部（四份探针金样各带一份，供无人值守核对）**：`key.tsv.gz`、`key_sequence.tsv.gz`、
  `key_sequence_tab.tsv.gz`、`sound_to_char_shape.tsv.gz` 的开头是 `#` 注释行，形如
  `# reference: <仓库> @ <40 位 pin>` + `# <来源文件> sha256: <64 位>`（`key.tsv.gz` 的来源文件是
  librime `key_table.cc`，另两份是 `lua/tiger_sentence.lua`，音反查金样另有 `PY_c.dict.yaml`）。
  这些值必须与本表的「来源与校验和」一致——换 pin 重生成后只改表、不改头部即被
  `tools/checks/verify_golden_shas.py` 拦下（**CI 两个作业各跑一次**：`rust` 作业校验表与头部，
  `golden` 作业另用 `--reference` 校验参照检出的 `lua/*`、`tools/*` 溯源）。

CI 以同一参照提交重生成全部 fixture 金样并与入库内容比对（见 `.github/workflows/ci.yml`）。
`key.tsv.gz`、`key_sequence.tsv.gz`、`key_sequence_tab.tsv.gz`、`sound_to_char_shape.tsv.gz` 依赖具体 librime/librime-lua 版本，**CI 不重生成**
（改由 CI 按上表校验其 sha256——内联 `sha256sum -c` 的三条与上面的校验器**互为独立来源**，两者都须通过）。
