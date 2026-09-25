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
- `uninstall.sh` 按同一清单删除（三项交互问答只管主题 / 模型 / 用户数据，不影响随包数据；
  此前枚举 7 个文件名而安装侧用 glob，`data/` 增删文件就会残留）。

一致性自检：`bash tools/checks/check_data_manifest.sh`（CI 已接入；`data/` 里
`tiger_sentence.*` 与 `symbols.yaml` 必须全部登记在清单里）。**清单保持纯 ASCII**：CMake 的
`file(STRINGS)` 默认编码会破坏非 ASCII 字节，把中文注释行拆成假文件名（configure 期即报错）。

不随包：n-gram 模型（用户自取；交互式卸载的「是否卸载模型」一问回答 y 才删）与本文件。

## 文件

- 码表四件套 `tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`：取自上游
  [`tiger-sentense-rime`](https://github.com/lvyww/tiger-sentense-rime)（GPL-3.0），与测试夹具
  `goldens/lexicon/` 同内容（`supplement.txt` 仅注释中的方案名由「虎整句」改为「虎句」；
  各文件 sha256 见 [`../goldens/regenerate.md`](../goldens/regenerate.md) 的夹具表）。
- `tiger_sentence.codes.huma.txt`：**追加码表**——虎码官方版（`2026.08.15`）单字表里**主表没有
  的字**（102,332 条、93,666 字），让生僻字可打：并上主表共 103,460 字（基本区 20,992 全覆盖、
  扩展 A 6,592、扩展 B–G 74,942，另有官方表一并编码的部首/笔画/注音等 934 个非汉字）。
  只补主表没有的字：给主表已有的字并官方短码会改它的最优码（`optimal_single` 由 true 变 false，
  「整串直出」奖励不再可达），那就不叫「只追加」了——主表已有字的拼写一律以虎句主表为准。
  源不入库；许可以 `LicenseRef-HuMa-Official` 如实记录（官方随包未声明形式化授权），
  生成与登记规则见下方「追加码表」。
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

## 追加码表（多表加载）

给方案补字 / 补码而**不动主表** ⇒ 可整块回滚、可逐张替换。内核规则
（`crates/hux-scheme/tiger/src/lexicon.rs`）：

- 主表 `tiger_sentence.codes.txt` 必需，且只取**第一个**命中的数据目录（用户目录覆盖共享目录）；
- 追加表 `tiger_sentence.codes.<name>.txt`（`<name>` 非空）**必须与主表同目录**，按文件名字典序
  拼在主表之后——别的数据目录里的码表不混进这份方案数据。**注意**：若你在用户数据目录放了自己的
  主表，追加表也要放在**同一个目录**才生效（否则共享目录里的 `huma` 表会被静默忽略、字集退回主表
  的 9,794 字）；实际装载了哪几张追加表可由 `Lexicon::extra_code_tables()` 查；
- 拼接后走同一条 `parse_codes_content`：`(text, code)` 去重保首见、同码内行序即 rank
  ⇒ **主表内所有 rank、`optimal_single` 等派生标志逐位不变**，追加表只能在既有码上垫后或引入新码；
- 追加表缺失 / 为空 / 不可读 / 是目录都不影响主表（只影响它自己）也不报错——所以「加张表」之后
  请用 `check_code_tables.py` 与 `shipped_data` 测试确认它真的生效；
- 每张追加表的 BOM 会被逐表剥掉（`normalize_text_content` 只剥得掉合并内容最前面那个）；
  仓库里的码表仍保持无 BOM。

新增一张追加表照下面走（现成例子：`tiger_sentence.codes.huma.txt`）：

1. **内容**：只写主表里**没有的字**的 `(字, 码)`——给主表已有的字并官方码会改它自己的最优码
   （`optimal_single` 由 true 变 false，「整串直出」奖励不再可达），就不是「只追加」了。
   格式与主表一致（每行 `<text>\t<code>`，`#` 与空行忽略，码只用小写 a–z）；同码内按发布方权重降序。
2. **登记四处**：`data/MANIFEST`（装 / 卸 / CMake 共用的唯一清单）、`REUSE.toml`（来源与许可标注）、
   `docs/resources.md` 的「1. 方案数据」表、`goldens/regenerate.md` 的 sha256 表（CI 逐行核对）。
3. **生成器**：`tools/generators/<name>.py`，读**不入库**的外部源（路径走环境变量，如 `HUMA_DICT`）、
   确定且幂等；表头写明源、版本与源文件 sha256，便于追溯。
4. **门槛**：`python3 tools/checks/check_code_tables.py`（命名口径 / 行格式 / 跨表去重 /
   「只补主表没有的字」）、`bash tools/checks/check_data_manifest.sh`、
   `python3 tools/checks/check_resources.py`、`python3 tools/checks/verify_golden_shas.py`，以及
   `cargo test -p hux-scheme-tiger --test shipped_data`（逐码比对「仅主表」与「主表 + 追加表」的
   装载结果：前缀、rank、`optimal_single` 全等）。
5. **回滚**：删掉该文件与上述四处登记即可，主表一个字节都不用改。

两条要知道的代价（现测数据：`huma` 表 102,332 条）：

- **装载成本**：release 库下仅主表约 17 ms / 常驻 19 MB，合并后约 269 ms / 107 MB（debug 更慢），
  且每次 rebuild（含 `apply_high_freq_limit`）都要重付；在意启动延迟时再考虑码表二进制化。
- **学习指纹**：码表内容（合并后）进 `sentence-v2|rules=<hash>`，所以追加表一改，既有的学习记录
  就不再命中（库文件还在，属一次「学习失效」体感）。
