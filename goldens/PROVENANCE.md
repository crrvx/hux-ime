<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# goldens：来源与校验和

- 本页＝两处 pin、sha256 表与内部头部口径。
- 机器可读来源：`tools/checks/verify_golden_shas.py`。
- 清单 / 格式见 [`README.md`](README.md)。
- 生成命令见 [`REGENERATE.md`](REGENERATE.md)。
- 两处 pin 都取自参照仓库。
- [`lvyww/tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime)
- **主干 pin**
  - `abad411750f79cfca750985fa266689b5d9b865f`
  - 该 pin 是 main 尖端。
  - 提交说明如下。
  - `fix(rime): preserve punctuation learning and default to full-m5`
- 由 Lua 核心生成的 16 份夹具 / decode / learning / lexical 金样都取自主干 pin。
- 两份键序列探针金样也取自主干 pin。
- 这两份是 `key_sequence.tsv.gz` 与 `key_sequence_tab.tsv.gz`。
- **反查分支 pin**
  - `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`
  - 该 pin 是 `feat/reverse-lookup` 尖端。
  - 尖端含两个提交：`4ff37c4` 数字选择器提交反查候选，`92a0b54` 撇号音节分隔。
  - 该 pin 是**主干 pin 的后代**（`abad411` 在其祖先链上）。
  - 因此音反查探针金样 `sound_to_char_shape.tsv.gz` 单独取自它。
  - 也**不再需要「分支 + 主干本地合并」**。
  - 生成器脚本已简化为单 `PIN` 加护栏。
  - 脚本为 `tools/generators/gen_sound_to_char_shape_golden.sh`。
  - 护栏：HEAD 必须等于 `PIN` 且工作区干净，否则显式失败。
- **键名表**
  - 来源是 librime `src/rime/key_table.cc`。
  - 该文件 sha256 见下。
  - `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`
  - 固定提交：`33e78140250125871856cdc5b42ddc6a5fcd3cd4`
  - `key_table.rs` 由 `tools/generators/gen_key_table.py` 生成。
  - CI 单文件下载源码后重生成比对。
  - `key.tsv.gz` 由系统 librime 1.17.0 探针生成，**CI 不重生成**。
  - 其内部头部记录本行的 pin 与 sha256。
  - 核对者：`tools/checks/verify_golden_shas.py`。
- **键序列 / 音反查 / Tab 锁**
  - 三份金样由 `tools/probes/rime_sequence_probe.cpp` 驱动。
  - 驱动对象是**真 librime + librime-lua**与对应的 pin 版 Lua 核心。
  - 探针头部记录参照提交与源文件 sha256。
  - 这三份金样**CI 不重生成**。
  - 夹具入库并与 Rust 重放共用。
  - 音反查夹具索引由 `tools/generators/gen_pinyin_index.py` 生成。
  - 该生成步骤在 CI 中重生成比对。
  - 音反查金样的撇号用例依赖上游 librime 的 delimiter 修复。
  - 该修复见 [rime/librime#1233](https://github.com/rime/librime/pull/1233)。
  - 即 `92a0b54` 的「按 `speller/delimiter` 切分音节」。
  - 本机 librime 1.17.0 未含该修复。
  - 故输入撇号后反查段**无候选**（金样如实记录）。
  - 本仓已实现该切分：撇号透明跳过、强制断音。
  - 预编辑里原样保留撇号、输入当场可见，段首/段尾同样保留。
  - 夹具索引补入 `xi`/`an` 后，三个 `apostrophe-*` 用例与金样确有差异。
  - 该差异已登记为差分测试 `DEVIATIONS` 的 `BranchPinDelimiter`。
  - 口径见 [`../docs/upstream-deviations.md`](../docs/upstream-deviations.md)。
  - 该册 ③ 节有登记。
  - 真实索引 `data/tiger_sentence.pinyin.bin.gz` 由同一生成器产出。
  - 其 sha256 是 `18a0931a…`。
  - 本地复验命令（`$REF` 为参照检出）见下。

    ```sh
    git -C "$REF" show 92a0b54:PY_c.dict.yaml > /tmp/PY_c.dict.yaml && python3 tools/generators/gen_pinyin_index.py --source /tmp/PY_c.dict.yaml --out /tmp/pinyin.bin.gz && cmp /tmp/pinyin.bin.gz data/tiger_sentence.pinyin.bin.gz
    ```

  - `92a0b54` 的反查分支含该文件，主干 pin 没有。
- **词先验**
  - `lexical.tsv.gz` 由 `tools/generators/gen_lexical_golden.lua` 生成。
  - 生成输入是参照 main（词先验模块自 `35a10b9` 起）与入库位图。
  - 许可证 CC BY 4.0，署名见 [`../docs/resources.md`](../docs/resources.md)。
  - 该金样**已在 CI 中再生成比对**。
- 参照仓库文件（生成时；`lua/`、`tools/` 均为参照仓库路径）。
- 两 pin 相同的文件只列一行。
- 各文件的来源 pin：

| 文件 | 来源 pin |
|---|---|
| `lua/tiger_sentence.lua`（主干金样） | 主干 `abad411` |
| `lua/tiger_sentence.lua`（音反查金样） | 反查 `92a0b54` |
| `lua/tiger_sentence_learning.lua` | 两 pin 相同 |
| `lua/tiger_sentence_ngram.lua` | 两 pin 相同 |
| `lua/tiger_sentence_cache.lua` | 两 pin 相同 |
| `lua/tiger_sentence_lexical.lua` | 两 pin 相同 |
| `tools/model_fixture.lua` | 主干 `abad411` |

- 上表各文件的 sha256（顺序同表）：

| 文件 | sha256 |
|---|---|
| `lua/tiger_sentence.lua`（主干金样） | `b77a747597a140e6fec315d8bc78344b8d8d3bdc007132bc7c53a9c6a3f22dd3` |
| `lua/tiger_sentence.lua`（音反查金样） | `f33cee28f78a612d77570297a6949732f46eeb3c4011b7fe760f43c3b3120b89` |
| `lua/tiger_sentence_learning.lua` | `0f685ae57fb4e70662492b7a3e56b91b5e8e9592cc64d881db181c9bf7acd9c6` |
| `lua/tiger_sentence_ngram.lua` | `fd7b2337d5215f51ffea092c76f07951a8e2172087e823d8a4b1641f11d8bf4e` |
| `lua/tiger_sentence_cache.lua` | `8ebd209588fb62d0bf888e752b95d8588ecbcdef2af40f8b009865fc3c41da7c` |
| `lua/tiger_sentence_lexical.lua` | `d49f45f0ee0033fd2466269d967b4784f508da220ec62215806e227ea590fe8d` |
| `tools/model_fixture.lua` | `ed5c771ee29835c20b46476635809ed37d70ad0c79d14df0ae13233f5da7d45a` |

- 数据夹具
  - `lexicon/` 四份取自参照仓库同名文件。
  - `key_sequence*/`、`sound_to_char_shape/` 为探针夹具，来源注记见各行。
- 各文件的来源与 sha256：

| 文件 | 来源 |
|---|---|
| `tiger_sentence.codes.txt` | 参照仓库同名文件 |
| `tiger_sentence.char_ranks.txt` | 参照仓库同名文件 |
| `tiger_sentence.full_code_whitelist.txt` | 参照仓库同名文件 |
| `tiger_sentence.supplement.txt` | 参照仓库同名文件，有本地改动 |
| `key_sequence/symbols.yaml` | 探针夹具 |
| `key_sequence_tab/symbols.yaml` | 探针夹具，与上同一文件 |
| `key_sequence_tab/tiger_sentence.custom.yaml` | 探针夹具，条件见下 |
| `sound_to_char_shape/symbols.yaml` | 探针夹具，与上同一文件 |
| `sound_to_char_shape/PY_c.dict.yaml` | 探针夹具，说明见下 |
| `sound_to_char_shape/tiger_sentence.codes.txt` | 探针夹具 |
| `sound_to_char_shape/tiger_sentence.pinyin.bin` | 生成物 |

- `tiger_sentence.supplement.txt` 有**本地改动**。
- 改动内容：仅注释中方案名「虎整句」→「虎句」。
- 该文件的上游 pin 为 `538f7d60…`，见 [`../data/README.md`](../data/README.md)。
- `key_sequence_tab/tiger_sentence.custom.yaml` 的夹具条件如下。
  - `tab_learning: true`
- `sound_to_char_shape/PY_c.dict.yaml` 是**本仓自建探针小词典**。
  - 规模：27 条 / 16 音节。
- 上游反查分支的同名文件是 82 万行真实词典，与本夹具无逐字节交集。
- `sound_to_char_shape/tiger_sentence.pinyin.bin` 是生成物。
- 上表各文件的 sha256（顺序同表）：

| 文件 | sha256 |
|---|---|
| `tiger_sentence.codes.txt` | `1d3e9b0ce0e4a603be3f220c71acecad846f020e87a52723ecb3814f6b53ac0e` |
| `tiger_sentence.char_ranks.txt` | `bd64e4bf333b2096a9a61fd5ece868e37912057bd1a812d75b2d5ccb4c994dcf` |
| `tiger_sentence.full_code_whitelist.txt` | `05d257457898146262f7dbf264103c70a8cf2ee92d188b770ad13232b293f566` |
| `tiger_sentence.supplement.txt` | `f229832bc92f89d87e4b1d29984aec53e627cedb23dda5074ad03cbcabdf0900` |
| `key_sequence/symbols.yaml` | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `key_sequence_tab/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `key_sequence_tab/tiger_sentence.custom.yaml` | `c610834a73205b5d584ad752c31b557df9547225e8924a9eb1fccc99cbd42ca7` |
| `sound_to_char_shape/symbols.yaml`（与上同一文件） | `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1` |
| `sound_to_char_shape/PY_c.dict.yaml` | `6664882e950edac066be09ffeecaae881a5388eed4835e2365196c83f2ffd177` |
| `sound_to_char_shape/tiger_sentence.codes.txt` | `4e2b7596db232e12ad997613155e067354652288270b55852d3fd2f16ab18709` |
| `sound_to_char_shape/tiger_sentence.pinyin.bin` | `cef711cadf2f2a28fe9df58bb133ffe42ea89749a42acbc5ab7f1f33a01d1556` |

- `lexicon_variants/` 与 `lexicon_codes_only/` 为人工构造的解析边界数据。
- 这两份无上游来源。

- 随包**追加码表**（只在 `data/`）
  - 源不入库。
  - 由 `tools/generators/merge_huma_codes.py` 从虎码官方版单字表生成。
  - 文件与 sha256：

| 文件 | sha256 |
|---|---|
| `data/tiger_sentence.codes.huma.txt` | `896f1aa2a302e33b6ec9beb8c516994c46e6a83fe6bd10b8cf9179c8d4a065a7` |

- 已入库金样 sha256

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
| `decode_learning_evidence.tsv.gz` | `86dec38d94f05e57de281e254262a40aef86176965d739afee6eb6b644464398` |
| `key.tsv.gz` | `7fae4983ab69e36ebd2e5cac267df81e9bc325731deaefcaa22873aeca8660c0` |
| `key_sequence.tsv.gz` | `8ce095cf98bae0bbfbd561e6b1e3aa7f1961af6e8e3c3b62b692a7e89e325aa1` |
| `key_sequence_tab.tsv.gz` | `3c89618d0fb067bb0ce5562646a62924576c0610226aed3fa70689b6549da9bc` |
| `sound_to_char_shape.tsv.gz` | `bcfe4102b66806b2f9b250d9679b7256a56bba6941c9190b41b07fd90d49500f` |
| `lexical.tsv.gz` | `5b559b2504e21c69b4f702678a96d2947abfe7d7c26adcd2b25c3d4de761e0c3` |

- **内部头部**（四份探针金样各带一份，供无人值守核对）
  - 开头 `#`注释行形如`# reference: <仓库> @ <40 位 pin>`
  - 另有形如 `# <来源文件> sha256: <64 位>` 的一行。
  - `key.tsv.gz` 的来源是 librime `key_table.cc`。
  - 另两份的来源是 `lua/tiger_sentence.lua`。
  - 音反查另有 `PY_c.dict.yaml`。
  - 头部必须与「来源与校验和」的表一致。
  - 换 pin 后只改表不改头部即被拦下。
  - 拦下者：`tools/checks/verify_golden_shas.py`。
  - **CI 两个作业各跑一次**。
  - `rust` 作业校验表与头部。
  - `golden` 作业另用 `--reference` 校验参照检出的 `lua/*`、`tools/*` 溯源。

- CI 以同一参照提交重生成全部 fixture 金样，并与入库内容比对。
- CI 定义见 `.github/workflows/ci.yml`。
- 四份探针金样依赖具体 librime/librime-lua 版本，**CI 不重生成**。
- 这四份探针金样如下。
  - `key`
  - `key_sequence`
  - `key_sequence_tab`
  - `sound_to_char_shape`
- 上列四份改按上表校验 sha256。
- 内联 4 处 `sha256sum -c`（共 7 条）与上面的校验器**互为独立来源**。
- 两者都须通过。
