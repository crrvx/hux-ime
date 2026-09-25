<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 资源总账：来源、作用、去向与校验

全仓资源的统一台账：每一项登记**来源**（项目 / 作者 / URL / pin 或提交 sha）、**作用**、
**许可**、**是否随包**、**默认去向**（系统级与用户级）与**校验方式**。

本页不参与安装：装 / 卸的实际输入是两份清单——数据 [`../data/MANIFEST`](../data/MANIFEST)、
主题 [`../assets/themes/MANIFEST`](../assets/themes/MANIFEST)；本页是给人看的账本，同时是机检对象：
`python3 tools/checks/check_resources.py` 核对「随仓存在的资源是否都登记在本页」与
「本页写的许可是否与 `REUSE.toml` 一致」，漏登记或写错许可即失败（CI 已接入）。
各分册：[`../data/README.md`](../data/README.md)、
[`../assets/branding/README.md`](../assets/branding/README.md)、
[`../assets/themes/README.md`](../assets/themes/README.md)、
[`../goldens/README.md`](../goldens/README.md)、
[`../goldens/regenerate.md`](../goldens/regenerate.md)、
[`LEXICAL_PRIOR_ATTRIBUTION.md`](LEXICAL_PRIOR_ATTRIBUTION.md)。

## 落点与查找顺序

- **系统级**：`<prefix>` = 安装前缀（`install.sh` 与 `cmake --install` 用 `/usr`）；插件本体
  `libhux.so` 落在 fcitx5 的 addon 目录（Debian/Ubuntu 为 `lib/<triplet>/fcitx5`，
  Arch/Fedora 为 `lib/fcitx5` 或 `lib64/fcitx5`）。
- **用户级**：数据 `$XDG_DATA_HOME/fcitx5/hux`（缺省 `~/.local/share/fcitx5/hux`），
  配置 `$XDG_CONFIG_HOME/fcitx5/conf/hux.conf`（缺省 `~/.config/fcitx5/conf/hux.conf`）。
- **查找顺序**（引擎）：`HUX_DATA_DIRS`（开发覆盖）→ 用户级 →
  `$XDG_DATA_DIRS/*/fcitx5/hux`（缺省 `/usr/local/share`、`/usr/share`）；
  模型另有 `HUX_MODEL` 覆盖。

## 1. 方案数据（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `data/tiger_sentence.codes.txt`<br>`data/tiger_sentence.char_ranks.txt`<br>`data/tiger_sentence.full_code_whitelist.txt`<br>`data/tiger_sentence.supplement.txt` | 虎整句（[tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)）作者与贡献者；主干 pin `abad411750f79cfca750985fa266689b5d9b865f`，取自上游同名文件（`supplement.txt` 仅注释里的方案名由「虎整句」改为「虎句」） | 字词码表 / 字频 / 全码白名单 / 补充词（词库解析的输入） | `GPL-3.0-only` | 是 | 系统级 `<prefix>/share/fcitx5/hux/`；用户级 `~/.local/share/fcitx5/hux/` | sha256 表（[`../goldens/regenerate.md`](../goldens/regenerate.md) 数据夹具表）+ `data/MANIFEST` 装后逐条核对 |
| `data/tiger_sentence.lexical.bin` | [rime-mohu](https://github.com/fcxxxz/rime-mohu) 的作者与贡献者；由上游 `mohu_flypy.base.dict.yaml` @ `9f43098cefdb450fe8dec0f3069fe8d9999b9d10` 经 `build_lexical_prior.py` 生成 | 紧凑词先验位图（TCSLEX01 Bloom filter，150,032 字节），整句排序用 | `CC-BY-4.0` | 是 | 同上 | sha256 `8dbc884b6cb719d07e4cef153c8048db19a11f8224f75a4ed87853e688a27393`（CI 校验；署名与复现见 [`LEXICAL_PRIOR_ATTRIBUTION.md`](LEXICAL_PRIOR_ATTRIBUTION.md)） |
| `data/tiger_sentence.codes.huma.txt` | 虎码官方（虎码输入法官方版）作者与贡献者；官方版 `2026.08.15` 的单字表（`publish/rime/tiger.dict.yaml`，sha256 `ca172d4e…`）里**主表没有的字**（102,332 条、93,666 字），源不入库 | 追加码表：生僻字可打（基本区全覆盖 + 扩展 A/B–G 等，主表 9,794 字 → 合计 10.3 万字）。内核把追加表拼在主表之后，故主表 rank 与简码分配、`optimal_single` 等派生标志都不变；删掉本文件即回滚 | `LicenseRef-HuMa-Official`（官方随包未声明形式化授权，如实记录，见 [`../LICENSES/LicenseRef-HuMa-Official.txt`](../LICENSES/LicenseRef-HuMa-Official.txt)） | 是 | 同上 | sha256 表（[`../goldens/regenerate.md`](../goldens/regenerate.md) 追加码表段）+ `data/MANIFEST` 装后逐条核对 + `python3 tools/checks/check_code_tables.py`（命名口径 / 行格式 / 跨表去重 / 只补主表没有的字）+ `cargo test -p hux-scheme-tiger --test shipped_data`（逐码比对「仅主表」与合并装载：前缀、rank、`optimal_single` 全等）；生成器 `tools/generators/merge_huma_codes.py` |
| `data/tiger_sentence.pinyin.bin.gz` | 虎整句（tiger-sentense-rime）作者与贡献者；上游 `PY_c.dict.yaml` @ 反查 pin `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`（该文件自 `898579f` 起未变，源自官方字词版 / 秃版小狼毫的简体拼音词典） | 音反查索引（TCSRV01：音节表 + 拼写表 + 按码分组的词条），反引号键音反查用 | `GPL-3.0-only` | 是 | 同上 | sha256 `18a0931a323ad58e1b13e610ec93183297118033d571ae303678613a217dad2a`（8,054,016 字节，CI 校验）；生成器 `tools/generators/gen_pinyin_index.py` 可用夹具重生成比对（CI 亦跑） |
| `data/symbols.yaml` | 虎整句（tiger-sentense-rime）作者与贡献者；上游 `symbols.yaml` @ 主干 pin `abad411750f79cfca750985fa266689b5d9b865f`（该文件自 `35a10b9` 以来未变，两个 pin 逐字节相同） | 标点表（`punctuator` 的 `half_shape` / `full_shape` 两组）；发布版仅覆盖 half_shape 的 `/`（参照原表为 `、`），full_shape 不变 | `GPL-3.0-only` | 是 | 同上 | 与参照原样夹具 `goldens/key_sequence/symbols.yaml`（sha256 `9b45c4a2f179d42585d5cc1439bfbcb5a585520f0de3ce83232180990e5cc9b1`）比对，差异只有上述一处；发布文件自身的 sha256 不在 CI 固定 |

**校验**：`bash tools/checks/check_data_manifest.sh`（清单 ↔ `data/` 实况 ↔ 装 / 卸 / CMake 三处一致）、
`python3 tools/checks/check_resources.py`（本页登记与许可）、
`python3 tools/checks/check_code_tables.py`（码表格式 + 跨表去重）、
`python3 tools/checks/verify_golden_shas.py`（含追加码表的 sha256 表），
以及 CI 里的两条 sha256（词先验、音反查索引）。
细节与不随包说明见 [`../data/README.md`](../data/README.md)。

## 2. 模型（不随包，用户自取）

| 资源（安装目录内文件名） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `models/sentence-ngram-mobile.bin` | 虎整句（tiger-sentense-rime）作者与贡献者；上游 [model release](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model) 或虎码 QQ 群 `948170058` | 三阶 KN 语言模型（TCSKNM02），整句排序打分 | `GPL-3.0-only` | 否 | 用户级 `~/.local/share/fcitx5/hux/models/`（推荐）；系统级 `<prefix>/share/fcitx5/hux/models/`；`HUX_MODEL` 可指向任意路径 | 文件头 magic `TCSKNM02` + 上游 Release 附件的 sha256（当前默认 `full-kn-m5-v2`：469,886,928 字节 / 448.12 MiB，sha256 `c0063898fdff27c1fb00c1c72fa28a6c1b375fade1ec2045d731b9db958bdecc`，另有 fused 214 MiB 变体）；状态菜单「模型」显示 `<文件名> — 已加载（三阶 TCSKNM02）` |
| `models/sentence-fivegram-mobile.bin` | 同上 | 五阶 KN 语言模型（TCSKNM03） | `GPL-3.0-only` | 否 | 同上 | 文件头 magic `TCSKNM03`（状态菜单按文件头标为「五阶 TCSKNM03」）；装载器只接受 TCSKNM02 mobile，默认查找也只找 `models/sentence-ngram-mobile.bin`——把五阶文件放进 `models/` 不会被默认命中，经 `HUX_MODEL` 指向它会以 `not a mobile TCSKNM02 model` 装载失败 |

**校验**：不随包，故无仓库内校验；放置后看状态菜单「模型」一行
（`已加载（三阶 TCSKNM02）` / `未找到模型（整句排序退化为码表名次）` / `装载失败：<原因>`），
选项与诊断说明见 [`config.md`](config.md)。安装脚本与 README 都只给下载入口与落点，不捆绑模型文件。

## 3. 共享图形（随包，Linux 安装规则取用）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `assets/branding/hux.png`<br>`assets/branding/hux.svg`<br>`assets/branding/hux-22.png`<br>`assets/branding/hux-48.png` | 本仓自绘（明雅流风）；`hux.png` 是**主源**（1600×1600 艺术位图），其余三个由它派生（`tools/generators/gen_branding_icons.py`） | 输入法条目与状态区图标：主源 + 自包含 SVG（内嵌主源，供支持 SVG 的主题）+ 22 / 48 px 位图 | `GPL-3.0-or-later` | 是（`hux.png` 仅作主源，不安装） | 系统级 `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/`（`hux.svg` 原名，两个位图安装时改名为 `hux.png`）；用户级（`install.sh -u`）同构于 `~/.local/share/icons/hicolor/…` | `python3 tools/checks/check_branding_assets.py`（主源 sha256、SVG 自包含且内嵌当前主源、位图边长与文件名一致、四文件聚合指纹；有 `rsvg-convert` 时查可渲染） |

**校验**：见上表；生成命令与各平台取用方式见
[`../assets/branding/README.md`](../assets/branding/README.md)。

## 4. 共享主题（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `assets/themes/hufu-canghai/`<br>`assets/themes/hufu-chenwu/`<br>`assets/themes/hufu-default/`<br>`assets/themes/hufu-huguang/`<br>`assets/themes/hufu-hupo/`<br>`assets/themes/hufu-luoxia/`<br>`assets/themes/hufu-mocha/`<br>`assets/themes/hufu-moyan/`<br>`assets/themes/hufu-mushan/`<br>`assets/themes/hufu-ouhe/`<br>`assets/themes/hufu-qingci/`<br>`assets/themes/hufu-rongyan/`<br>`assets/themes/hufu-shiyou/`<br>`assets/themes/hufu-songyan/`<br>`assets/themes/hufu-sujian/`<br>`assets/themes/hufu-xingyu/`<br>`assets/themes/hufu-xuanmo/`<br>`assets/themes/hufu-yingxiong/`<br>`assets/themes/hufu-yuebai/` | 虎符（[hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust)）作者与贡献者；其官方皮肤的 fcitx5 主题转换产物（取用自上游 `54c0339`），原样入库（每套 `theme.conf` + 6 张 PNG） | 19 套候选窗口主题（面板 / 高亮 / 翻页箭头 / 单选图） | `GPL-3.0-only` | 是 | 系统级 `<prefix>/share/fcitx5/themes/<主题目录>/`；用户级 `~/.local/share/fcitx5/themes/<主题目录>/`（fcitx5 合并两侧目录） | `assets/themes/MANIFEST` ↔ 目录 ↔ 文件集合 + 取用指纹 `7ad673c4c6df5330db8fc84566a65b93ab39c6686de12428f7caa208208c7a9d`（`python3 tools/checks/check_themes.py`） |

**校验**：`python3 tools/checks/check_themes.py`（清单 / 目录 / 文件 / `theme.conf` 引用图 / 聚合指纹）；
更新步骤见 [`../assets/themes/README.md`](../assets/themes/README.md)。

## 5. 测试金样与夹具（不随包）

| 资源（仓库路径 / glob） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `goldens/**` | 上游 Lua 核心（主干 pin `abad411750f79cfca750985fa266689b5d9b865f`）生成的夹具 / decode / learning / lexical 金样；真 librime 探针录制的 `key`、`key_sequence`、`key_sequence_tab`、`sound_to_char_shape`（音反查取反查 pin `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c`）；本项目自造的解析边界夹具（`lexicon_variants/`、`lexicon_codes_only/`）与合成码表 / `symbols.yaml` / `ngram_fixture.bin` | 差分 oracle：Rust 侧逐位重放比对，判定移植是否等价；不进运行时 | `GPL-3.0-only` | 否 | 无（只在仓库内被测试读取；真实模型抽样金样 `goldens/local/` 不入库） | sha256 表 + 金样内部头部 + 参照检出溯源（`python3 tools/checks/verify_golden_shas.py [--reference _external/tiger-sentense-rime]`）；夹具类金样在 CI 里重生成逐字节比对 |
| `goldens/key.tsv.gz` | RIME Developers；由系统 librime 1.17.0 的 `tools/probes/key_probe.cpp` 录制，键名表源自 librime `src/rime/key_table.cc` @ `33e78140250125871856cdc5b42ddc6a5fcd3cd4` | 键名 / 键事件金样（`name`/`repr`/`parse`/`modifier`） | `BSD-3-Clause` | 否 | 无（同上级目录） | sha256 `7fae4983ab69e36ebd2e5cac267df81e9bc325731deaefcaa22873aeca8660c0`（CI 内联校验，不重生成；内部头部另由 `verify_golden_shas.py` 核对） |

**校验**：`python3 tools/checks/verify_golden_shas.py`（表 ↔ 文件 ↔ 内部头部）；
再加 `--reference _external/tiger-sentense-rime` 核对参照检出里的 `lua/*`、`tools/*`。
清单、transcript 格式与「金样不得因有意偏离而重生成」的规则见
[`../goldens/README.md`](../goldens/README.md)，重生成命令全表见
[`../goldens/regenerate.md`](../goldens/regenerate.md)。

## 6. 源码生成物（不随包，编进插件）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `crates/hux-core/src/key_table.rs` | RIME Developers；由 librime `src/rime/key_table.cc` @ `33e78140250125871856cdc5b42ddc6a5fcd3cd4`（sha256 `2f7c6a8b4f2aa474d700a87bd4bd1baa48a2655cd6ce4d2ba05b768f284d9d78`）经 `tools/generators/gen_key_table.py` 生成，请勿手改 | 键名与修饰位表（键解析与键金样共用） | `BSD-3-Clause` | 否（编进 `libhux.so`） | 无 | CI 下载该 pin 的单文件后重生成并 `diff` 比对；文件头部自述来源 sha256 |

**校验**：`python3 tools/generators/gen_key_table.py --source <librime>/src/rime/key_table.cc --out /tmp/key_table.rs && diff /tmp/key_table.rs crates/hux-core/src/key_table.rs`
（CI 的 `golden` 作业执行同一比对）。

## 7. 文档图片（不随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `docs/images/虎句.png`<br>`docs/images/虍.png`<br>`docs/images/配置页.png`<br>`docs/images/音反查.png`<br>`docs/images/字反查.png` | 本仓截图（明雅流风） | README 与文档的插图（候选窗口 / 图标 / 配置页 / 反查） | `GPL-3.0-or-later` | 否 | 无 | 无内容校验，仅按 [`../REUSE.toml`](../REUSE.toml) 目录标注许可 |

## 8. 插件元数据与配置（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `platform/fcitx5/conf/hux.addon.conf`<br>`platform/fcitx5/conf/hux.inputmethod.conf` | 本仓（明雅流风） | fcitx5 addon 元数据（`Category=InputMethod`、`Library=libhux`、`OnDemand`）与输入法条目 | `GPL-3.0-or-later` | 是 | 系统级 `<prefix>/share/fcitx5/addon/hux.conf` 与 `<prefix>/share/fcitx5/inputmethod/hux.conf`（安装时都改名为 `hux.conf`）；用户级（`install.sh -u`）`~/.local/share/fcitx5/{addon,inputmethod}/hux.conf` | `cmake --install` 落点核对（CI 的 `addon` 作业按 `DESTDIR` 检查两个 conf）+ `uninstall.sh` 按固定路径删除 |

## 9. 许可证文本

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `LICENSE` | 本仓（明雅流风） | 项目许可正文（GPL-3.0，供 GitHub 识别） | `GPL-3.0-or-later` | 否 | 无 | `reuse lint`（REUSE 规范忽略根许可正文） |
| `LICENSES/GPL-3.0-or-later.txt`<br>`LICENSES/GPL-3.0-only.txt`<br>`LICENSES/CC-BY-4.0.txt`<br>`LICENSES/BSD-3-Clause.txt`<br>`LICENSES/LicenseRef-HuMa-Official.txt` | SPDX 官方许可正文镜像（各许可的发布方）；`LicenseRef-HuMa-Official.txt` 是本仓自写的情况说明（虎码官方随包未声明形式化授权） | 各许可全文：项目代码 / 上游派生数据 / 词先验 / 键名表；以及追加码表的授权口径说明 | `GPL-3.0-or-later`<br>`GPL-3.0-only`<br>`CC-BY-4.0`<br>`BSD-3-Clause`<br>`LicenseRef-HuMa-Official` | 否 | 无 | `reuse lint` + 与各文件 SPDX 头 / [`../REUSE.toml`](../REUSE.toml) 标注对应 |
| `REUSE.toml` | 本仓（明雅流风） | 无 SPDX 头的二进制 / 第三方文件的许可与版权标注（REUSE 规范） | `GPL-3.0-or-later` | 否 | 无 | `reuse lint`；许可一致性另由 `python3 tools/checks/check_resources.py` 与本页比对 |

**校验**：`reuse lint`（CI 的 `reuse` 作业）。逐文件许可标注以后者为准；本页写的是各类资源的**许可归属**。

## 10. 运行时可写数据（不随包，卸载对账用）

| 资源（默认路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `tiger_sentence.options.yaml` | 引擎运行时写入（方案选项持久化） | 状态菜单 / 配置页切换的选项落盘（合并顺序：本文件 > 设置 > 内建缺省） | `GPL-3.0-or-later`（运行时产物） | 否 | 用户级 `~/.local/share/fcitx5/hux/tiger_sentence.options.yaml` | 无内容校验；交互式卸载的「是否删除用户数据」一问回答 y 才删 |
| `tiger_sentence_learning_<hash>.userdb/` | 引擎运行时写入（LevelDB 学习库） | 用户选词学习与打分（键 `e/%010d`；`<hash>` = 方案 id 哈希） | `GPL-3.0-or-later`（运行时产物） | 否 | 用户级 `~/.local/share/fcitx5/hux/tiger_sentence_learning_<hash>.userdb/` | 无内容校验；同一问回答 y 才删（缺省保留） |
| `conf/hux.conf` | fcitx5 配置页与状态菜单写入 | 配置项落盘（配置页「虎虚」页；与选项存储双向同步、切换即时生效） | 无（用户配置） | 否 | 用户级 `~/.config/fcitx5/conf/hux.conf` | 无内容校验；同一问回答 y 才删 |

**校验**：三者都不是随包资源，不进 `data/MANIFEST`；此处登记只为「卸载干净」对账
（`./uninstall.sh` 的「是否删除用户数据」一问回答 `n` 即保留、回答 `y` 即清除），精确落点与语义见
[`config.md`](config.md) 与 [`usage.md`](usage.md)。
安装脚本本身**不写**任何持久文件（除 `-u` 的 `~/.config/environment.d/90-hux.conf`，见下）。

## 10b. 安装模式写入的配置（仅 `install.sh -u`）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 |
| --- | --- | --- | --- | --- | --- | --- |
| `~/.config/environment.d/90-hux.conf` | 本仓安装脚本生成（非仓库文件） | 让 systemd 用户实例为 fcitx5 带上 `FCITX_ADDON_DIRS=$HOME/.local/lib/fcitx5:/usr/lib/fcitx5`——fcitx5 的 addon 库目录**没有用户级缺省值**，只有该变量能让用户级插件被加载（它会**取代**缺省值，故必须显式带上系统目录） | 无（用户环境配置） | 否 | 用户级 `~/.config/environment.d/90-hux.conf` | 写入时内容幂等（相同则不改写）；`uninstall.sh` 删除；生效需重新登录或启动前 export |

两种安装模式（`-s` 系统级 `<prefix>=/usr`、`-u` 用户级 `<prefix>=$HOME/.local`）的落点与前提见
[`usage.md`](usage.md)；用户级模式下上表第 1 / 3 / 4 / 8 节的 `<prefix>` 即 `$HOME/.local`。

## 11. 后台服务与端口

**本引擎无常驻服务、无 socket、无端口**：

- addon 是 fcitx5 进程内的动态库（`libhux.so`，`OnDemand=True` 按需加载）：没有自己的守护进程，
  不监听端口，也不与任何外部进程或服务通信（有的实现把引擎放到独立进程再经 socket / IPC 通信，
  本仓没有这条路径）。
- 学习库是同进程内的 LevelDB（`…userdb/`），只用文件锁，不占端口。
- 诊断日志走 fcitx5 自身的日志设施，类别 `hux`；安装脚本不再自动重启 fcitx5（结尾只给重启命令），
  因此不产生脚本侧的日志文件。
- 开发 / 部署期的进程环境覆盖只有 `HUX_DATA_DIRS`（数据目录）与 `HUX_MODEL`（模型路径）；
  其余 `HUX_*` 名字只出现在测试与探针里。

## 12. 未随包 / 未使用的第三方

| 项目 | 用途 | 现状 |
| --- | --- | --- |
| [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime) 的 Lua 核心 | 测试 oracle：生成金样、对照语义 | 不进运行时；检出不入库（本地 `_external/`，CI 自建临时检出后弃用） |
| librime / librime-lua | 生成金样的探针（`tools/probes/*.cpp`）与键名表来源 | 运行时不链接、不依赖；仅测试期与生成期使用 |
| [rime-mohu](https://github.com/fcxxxz/rime-mohu) | 词先验位图的上游词库 | 只经派生位图间接使用（`CC-BY-4.0`，署名见 [`LEXICAL_PRIOR_ATTRIBUTION.md`](LEXICAL_PRIOR_ATTRIBUTION.md)） |
| [hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust) | 共享主题的来源 | 只用转换后的主题产物（`assets/themes/`），不引入其代码 |
| 真实 n-gram 模型与本地抽样金样（`goldens/local/`） | 性能基准与真实模型差分 | 均不入库：模型用户自取，抽样金样本地生成 |

**校验**：依赖边界由 CI 的层依赖守卫与 `cargo tree` 判定（内核不依赖方案、平台不直接引用方案内部模块），
见 [`refactor.md`](refactor.md) §7。

## 自动校验

```sh
python3 tools/checks/check_resources.py     # 本页登记 ↔ 随仓资源 ↔ REUSE.toml 许可
bash tools/checks/check_data_manifest.sh    # 随包数据清单 ↔ data/ ↔ 装 / 卸 / CMake
python3 tools/checks/check_themes.py        # 主题清单 ↔ 目录 ↔ 指纹
python3 tools/checks/check_branding_assets.py
python3 tools/checks/verify_golden_shas.py [--reference _external/tiger-sentense-rime]
reuse lint
```

新增随包资源（数据 / 主题 / 图形 / 插件 conf）时：先落清单（`data/MANIFEST` 或
`assets/themes/MANIFEST`），再在本页补一行；新增文档图片、许可证文本同样要在本页补一行。
只落清单不补本页会被 `check_resources.py` 拦下（随包资源与上述几类要求逐条登记，
宽 glob 不作为登记；`goldens/**` 这类测试金样按组登记即可）。
