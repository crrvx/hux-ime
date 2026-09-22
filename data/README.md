<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# data/

运行时数据（随包安装到 `…/hux/`）。

## 安装清单：`MANIFEST`（单一来源）

`MANIFEST` 每行一个仓库相对路径（`#` 行是注释），是**装 / 卸 / CMake 三处共用的唯一清单**：

- `platform/fcitx5/CMakeLists.txt` 的 `install(FILES …)` 按它安装（**只用 `cmake --install`
  也能得到完整引擎**——此前 CMake 不装数据，只走 CMake 会得到「无词库」引擎；见
  [`../platform/fcitx5/README.md`](../platform/fcitx5/README.md) ）；
- `install.sh` 装后逐条核对落盘（缺任一即失败并给出提示）；
- `uninstall.sh` 无 `--purge` 时按同一清单删除（此前枚举 7 个文件名而安装侧用 glob，
  `data/` 增删文件就会残留）。

一致性自检：`bash tools/checks/check_data_manifest.sh`（CI 已接入；`data/` 里
`tiger_sentence.*` 与 `symbols.yaml` 必须全部登记在清单里）。**清单保持纯 ASCII**：CMake 的
`file(STRINGS)` 默认编码会破坏非 ASCII 字节，把中文注释行拆成假文件名（configure 期即报错）。

不随包：n-gram 模型（用户自取；`uninstall.sh --purge` 才删）与本文件。

## 文件

- 码表四件套 `tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`：取自上游
  [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime)（GPL-3.0），与测试夹具
  `goldens/lexicon/` 同内容（`supplement.txt` 仅注释中的方案名由「虎整句」改为「虎句」；
  各文件 sha256 见 [`../goldens/regenerate.md`](../goldens/regenerate.md) 的夹具表）。
- `tiger_sentence.lexical.bin`：紧凑词先验（TCSLEX01 Bloom filter）。
  来源、许可（CC BY 4.0）、参数与校验和见
  [`../docs/LEXICAL_PRIOR_ATTRIBUTION.md`](../docs/LEXICAL_PRIOR_ATTRIBUTION.md)。
- `tiger_sentence.pinyin.bin.gz`：音反查索引（TCSRV01：音节表 + 拼写表（本体/缩写）+
  按码分组的词条）。由 `tools/generators/gen_pinyin_index.py` 从参照实现
  [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime) 的 `PY_c.dict.yaml`（提交
  `92a0b54`＝反查分支尖端；该文件自 `898579f` 起未变，源自官方字词版/秃版小狼毫的简体拼音词典）转换而来（8,054,016 字节，
  sha256 `18a0931a…`）；语义与接线见 [`../docs/design.md`](../docs/design.md)。
- `symbols.yaml`：标点表（`punctuator/half_shape|full_shape`；`{commit}`/标量/`{pair}`）。
  取自参照 `symbols.yaml`（主干 pin `abad411`；该文件自 `35a10b9` 以来未变，两个 pin 逐字节相同），仅覆盖一处默认：half_shape 的 `"/"` 提交 `"/"`
  （参照原表为 `、`）；full_shape 不变。测试夹具 `goldens/key_sequence/symbols.yaml` 保持参照原样。
