<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# data/

运行时数据（随包安装到 `…/hux/`）。

- 码表四件套 `tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`：取自上游
  [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime)（GPL-3.0），与测试夹具
  `goldens/lexicon/` 同内容（`supplement.txt` 仅注释中的方案名由「虎整句」改为「虎句」；
  各文件 sha256 见 [`../goldens/README.md`](../goldens/README.md) 的夹具表）。
- `tiger_sentence.lexical.bin`：紧凑词先验（TCSLEX01 Bloom filter）。
  来源、许可（CC BY 4.0）、参数与校验和见
  [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)。
- `tiger_sentence.pinyin.bin.gz`：音查虎索引（TCSRV01：音节表 + 拼写表（本体/缩写）+
  按码分组的词条）。由 `tools/generators/gen_pinyin_index.py` 从参照实现
  [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 的 `PY_c.dict.yaml`（提交
  `898579f`，源自官方字词版/秃版小狼毫的简体拼音词典）转换而来（8,054,016 字节，
  sha256 `18a0931a…`）；语义与接线见 [`../docs/rust-migration.md`](../docs/rust-migration.md)。
- `symbols.yaml`：标点表（`punctuator/half_shape|full_shape`；`{commit}`/标量/`{pair}`）。
  取自参照 `symbols.yaml`（pin `8b615235`；该文件自 `35a10b9` 以来未变），仅覆盖一处默认：half_shape 的 `"/"` 提交 `"/"`
  （参照原表为 `、`）；full_shape 不变。测试夹具 `goldens/key_sequence/symbols.yaml` 保持参照原样。
