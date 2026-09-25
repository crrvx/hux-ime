<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 资源总账：来源、作用、去向、再生与校验

全仓资源台账，栏目即七要素（**来源** = 项目 / 作者 / URL / pin 或 sha256； \
**校验** = 守卫命令 + CI 作业（映射见末节「自动校验」）。 \
操作步骤归各分册（[`../data/README.md`](../data/README.md)、 \
[`../assets/branding/README.md`](../assets/branding/README.md)、 \
[`../assets/themes/README.md`](../assets/themes/README.md)、 \
[`../goldens/README.md`](../goldens/README.md)），本页只登记与链接； \
安装的输入是 [`../data/MANIFEST`](../data/MANIFEST) 与 \
[`../assets/themes/MANIFEST`](../assets/themes/MANIFEST)，不是本页。 \
本页被 `python3 tools/checks/check_resources.py`（CI `rust`）机检： \
随仓资源须逐条登记、许可须与 [`../REUSE.toml`](../REUSE.toml) 一致。

## pin 与出处

| 名称 | 出处 | pin / sha256 | 提供 |
| --- | --- | --- | --- |
| 主干 pin | 虎爪-rime（虎句参照）<br>[tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime) main 尖端 | `abad411750f79cfca750985fa266689b5d9b865f` | 码表四件套与 `symbols.yaml`；Lua 金样；键序列 / Tab 锁探针金样 |
| 反查 pin | 同上 `feat/reverse-lookup` 尖端（主干 pin 后代，`abad411` 在祖先链上） | `92a0b54b53114e7e5aa6a1ff48efa95db0e21f9c` | 音反查源 `PY_c.dict.yaml`（自 `898579f` 未变）；音反查探针金样与夹具 |
| 词先验上游 | [rime-mohu](https://github.com/fcxxxz/rime-mohu) | `9f43098cefdb450fe8dec0f3069fe8d9999b9d10` | 源词库 `mohu_flypy.base.dict.yaml`（见下节） |
| 键名表上游 | [librime](https://github.com/rime/librime) | `33e78140250125871856cdc5b42ddc6a5fcd3cd4` | `src/rime/key_table.cc` |
| 主题上游 | 虎符（[hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust)） | `54c0339` | 官方皮肤的 fcitx5 主题转换产物（19 套） |
| 追加码表上游 | 虎码官方版 `2026.08.15` 的 `publish/rime/tiger.dict.yaml` | `ca172d4e55006bec389b6e3876bb08d4db1e1d1ae49a64f5d9d2c7d8c5e3abe5` | 主表没有的 `(字, 码)` 对（源不入库） |

## 落点与查找顺序

**账本口径**：系统级 `<prefix>`（`-s` / `cmake --install` 用 `/usr`）与用户级去向见表内列； \
`libhux.so` 随 fcitx5 addon 目录。 \
安装落点与目录查找顺序见 [`usage.md`](usage.md) 与 [`design.md`](design.md) §7（单一来源）。

## 1. 方案数据（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `data/tiger_sentence.codes.txt`<br>`data/tiger_sentence.char_ranks.txt`<br>`data/tiger_sentence.full_code_whitelist.txt`<br>`data/tiger_sentence.supplement.txt` | 「主干 pin」上游同名文件（作者见「pin 与出处」）<br>`supplement.txt` 仅注释里方案名改为「虎句」 | 字词码表 / 字频 / 全码白名单 / 补充词 | `GPL-3.0-only` | 是 | 系统级 `<prefix>/share/fcitx5/hux/`<br>用户级 `~/.local/share/fcitx5/hux/` | sha256 见 [`../goldens/README.md`](../goldens/README.md)「数据夹具」表 | 随「主干 pin」重取同名文件；见 [`../data/README.md`](../data/README.md) |
| `data/tiger_sentence.lexical.bin` | [rime-mohu](https://github.com/fcxxxz/rime-mohu)<br>由「词先验上游」源词库生成 | 紧凑词先验位图（TCSLEX01），整句排序用 | `CC-BY-4.0` | 是 | 同上 | sha256 见下节；参数见 [`../data/README.md`](../data/README.md) | 上游 `tools/build_lexical_prior.py`；步骤见下节 |
| `data/tiger_sentence.codes.huma.txt` | 由「追加码表上游」单字表里**主表没有的字**生成（源不入库） | 追加码表：生僻字可打；见下 | `LicenseRef-HuMa-Official` | 是 | 同上 | sha256 见 [`../goldens/README.md`](../goldens/README.md)「随包追加码表」表 | `tools/generators/merge_huma_codes.py`（源不入库）<br>回滚与登记见 [`../data/README.md`](../data/README.md)「追加码表」 |
| `data/tiger_sentence.pinyin.bin.gz` | 由「反查 pin」的 `PY_c.dict.yaml` 生成<br>（自 `898579f` 未变，源自官方字词版 / 秃版小狼毫） | 音反查索引（TCSRV01），反引号键音反查 | `GPL-3.0-only` | 是 | 同上 | sha256 `18a0931a323ad58e1b13e610ec93183297118033d571ae303678613a217dad2a` | `tools/generators/gen_pinyin_index.py`<br>从上游 `PY_c.dict.yaml` 重建（重生成与逐字节比对见 [`../goldens/README.md`](../goldens/README.md)） |
| `data/symbols.yaml` | 「主干 pin」上游 `symbols.yaml`<br>（自 `35a10b9` 未变，两个 pin 逐字节相同） | 标点表（`punctuator` 的 `half_shape` / `full_shape`）<br>发布版仅覆盖 half_shape 的 `/`（参照 `、`） | `GPL-3.0-only` | 是 | 同上 | 发布文件 sha256 `6011dc59464cf915cfe5a509c1030924c02c23bbb6f429b0a24516fe9809a239`（**不在 CI 固定**）<br>夹具 `goldens/key_sequence/symbols.yaml` 保持参照原样（`9b45c4a2…`，`verify_golden_shas.py`） | 随「主干 pin」重取上游 `symbols.yaml`，重做 `/` 一处默认 |

- 四件套 sha256 见 [`../goldens/README.md`](../goldens/README.md)「数据夹具」表； \
  其中 `supplement.txt` 是唯一的本地改动（重取上游后须重做）。
- 追加码表只增不改：拼在主表之后，主表 rank 与 `optimal_single` 等派生标志不变， \
  删该文件即回滚；条数与门槛（`check_code_tables.py`、`shipped_data` 逐码比对）见 \
  [`../data/README.md`](../data/README.md)「追加码表」。
- `LicenseRef-HuMa-Official`：官方随包未声明形式化授权，如实记录； \
  原文见 [`../LICENSES/LicenseRef-HuMa-Official.txt`](../LICENSES/LicenseRef-HuMa-Official.txt)（§9）。

## 词先验：署名与复现

`data/tiger_sentence.lexical.bin`（TCSLEX01， \
sha256 `8dbc884b6cb719d07e4cef153c8048db19a11f8224f75a4ed87853e688a27393`） \
是紧凑排序先验的词存在性位图（**不含词文本与权重**）；源词库不入库； \
参数与口径见 [`../data/README.md`](../data/README.md)。

- 项目 / 作者：[`fcxxxz/rime-mohu`](https://github.com/fcxxxz/rime-mohu) contributors； \
  源文件 `mohu_flypy.base.dict.yaml` @ `9f43098…`（见「词先验上游」）， \
  原文件 SHA-256 `877c6dacb4d5bb6738e230ce2d9235f3ac0f48404959c2db26fb18c7ddd31cb6`。
- 源数据：Rime 八股文词库、THUOCL（依其原许可再发行）、雾凇拼音补充数据及人工补充词。
- 许可：原文件声明 **CC BY 4.0**（原项目： \
  「完整方案按 GPL-3.0 发布，文件另有声明时以文件声明为准」）； \
  正文见[`../LICENSES/CC-BY-4.0.txt`](../LICENSES/CC-BY-4.0.txt)、 \
  <https://creativecommons.org/licenses/by/4.0/>。
- 本仓**原样沿用**该位图（sha256 与上游一致），仅由参照仓库根移到 `data/`； \
  Rust 侧读取与打分在 `crates/hux-scheme/tiger/src/lexical.rs`； \
  转换与移植不表示上游作者认可本项目。

**复现**：取得源词库后，在参照检出（放 `_external/`，见 [`../AGENTS.md`](../AGENTS.md)）\
内用上游脚本：

```sh
git clone https://github.com/fcxxxz/rime-mohu _external/rime-mohu
(
  cd _external/tiger-sentense-rime
  python3 tools/build_lexical_prior.py \
    --source ../rime-mohu/mohu_flypy.base.dict.yaml \
    --source-repository https://github.com/fcxxxz/rime-mohu \
    --source-revision 9f43098cefdb450fe8dec0f3069fe8d9999b9d10 \
    --source-license CC-BY-4.0 \
    --codes tiger_sentence.codes.txt \
    --output tiger_sentence.lexical.bin \
    --manifest /tmp/lexical.manifest.json
)
```

产物应与 `data/tiger_sentence.lexical.bin`（sha256 见上）一致，`--manifest` 可留作对拍。

## 2. 模型（不随包，用户自取）

| 资源（安装目录内文件名） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `models/sentence-ngram-mobile.bin` | [model release](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model)<br>（上游）或虎码 QQ 群 `948170058` | 三阶 KN 语言模型（TCSKNM02），整句打分 | `GPL-3.0-only` | 否 | 用户级 `~/.local/share/fcitx5/hux/models/`（推荐）<br>系统级 `<prefix>/share/fcitx5/hux/models/`<br>`HUX_MODEL` 可覆盖 | magic `TCSKNM02` + Release 附件 sha256（见下） | 用户自取后放到 `models/` 下 |
| `models/sentence-fivegram-mobile.bin` | 同上 | 五阶 KN 语言模型（TCSKNM03） | `GPL-3.0-only` | 否 | 同上 | magic `TCSKNM03`；只接受三阶 mobile（见下） | 同上 |

- 不随包、无仓库内校验；README 与安装脚本只给入口与落点，不捆绑模型文件。
- 默认模型 `full-kn-m5-v2`：469,886,928 字节 / 448.12 MiB， \
  sha256 `c0063898fdff27c1fb00c1c72fa28a6c1b375fade1ec2045d731b9db958bdecc`； \
  另有 fused 214 MiB 变体。
- 装载器只接受 TCSKNM02 mobile、默认只找 `models/sentence-ngram-mobile.bin`； \
  五阶文件放进 `models/` 不命中，经 `HUX_MODEL` 指向它以 `not a mobile TCSKNM02 model` 失败； \
  状态菜单按文件头标为「五阶 TCSKNM03」，诊断见 [`config.md`](config.md)。

## 3. 共享图形（随包，Linux 安装规则取用）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `assets/branding/hux.png`<br>`assets/branding/hux.svg`<br>`assets/branding/hux-22.png`<br>`assets/branding/hux-48.png` | 本仓自绘（明雅流风）；`hux.png` 是**主源**（1600×1600 艺术位图）<br>其余由它派生 | 输入法条目与状态区图标 | `GPL-3.0-or-later` | 是（`hux.png` 仅主源，不安装） | 系统级 `<prefix>/share/icons/hicolor/{scalable,48x48,22x22}/apps/`<br>用户级（`-u`）同构 `~/.local/share/icons/…`<br>（`hux.svg` 原名，位图安装时改名 `hux.png`） | `python3 tools/checks/check_branding_assets.py`（CI `rust`） | `tools/generators/gen_branding_icons.py`（勿手改派生文件）<br>取用见 [`../assets/branding/README.md`](../assets/branding/README.md) |

- 主源 sha256：`dbb46e2b6700589a9634aef0918630f2f48d3d5effa824d529271c688a605036`。
- 四文件聚合指纹：`9bc316e2bee17e62514d4c091a1996d85f5d1b3dc9be631d95f96072c550c6df`。

## 4. 共享主题（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `assets/themes/hufu-canghai/`<br>`assets/themes/hufu-chenwu/`<br>`assets/themes/hufu-default/`<br>`assets/themes/hufu-huguang/`<br>`assets/themes/hufu-hupo/`<br>`assets/themes/hufu-luoxia/`<br>`assets/themes/hufu-mocha/`<br>`assets/themes/hufu-moyan/`<br>`assets/themes/hufu-mushan/`<br>`assets/themes/hufu-ouhe/`<br>`assets/themes/hufu-qingci/`<br>`assets/themes/hufu-rongyan/`<br>`assets/themes/hufu-shiyou/`<br>`assets/themes/hufu-songyan/`<br>`assets/themes/hufu-sujian/`<br>`assets/themes/hufu-xingyu/`<br>`assets/themes/hufu-xuanmo/`<br>`assets/themes/hufu-yingxiong/`<br>`assets/themes/hufu-yuebai/` | 虎符（[hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust)）作者与贡献者<br>官方皮肤的主题转换产物（见下） | 19 套候选窗口主题（每套 `theme.conf` + 6 张 PNG） | `GPL-3.0-only` | 是 | 系统级 `<prefix>/share/fcitx5/themes/<主题目录>/`<br>用户级 `~/.local/share/fcitx5/themes/<主题目录>/`<br>（fcitx5 合并两侧目录） | 取用指纹 `7ad673c4…`<br>`python3 tools/checks/check_themes.py`（CI `rust`） | 虎符仓库重生成后覆盖本目录并同步 `MANIFEST`<br>见 [`../assets/themes/README.md`](../assets/themes/README.md) |

- 取用指纹（133 个文件的聚合 sha256）、文件集合与更新步骤见 \
  [`../assets/themes/README.md`](../assets/themes/README.md)。

## 5. 测试金样与夹具（不随包）

| 资源（仓库路径 / glob） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `goldens/**` | 「主干 pin」Lua 核心生成的金样<br>真 librime 探针录制的四份金样（音反查取「反查 pin」）<br>本项目自造的解析边界夹具与合成码表 / `symbols.yaml` | 差分 oracle：逐位重放比对；不进运行时 | `GPL-3.0-only` | 否 | 无（只在仓库内被测试读取；`goldens/local/` 为本地抽样金样，不入库、不随包） | sha256 表 + 内部头部 + 参照溯源（`verify_golden_shas.py`；`--reference` 另核检出） | 重生成命令与规则见 [`../goldens/README.md`](../goldens/README.md) |
| `goldens/key.tsv.gz` | RIME Developers；系统 librime 1.17.0 的 `tools/probes/key_probe.cpp` 录制<br>键名表源自 librime `src/rime/key_table.cc` @「键名表上游」 | 键名 / 键事件金样（`name`/`repr`/`parse`/`modifier`） | `BSD-3-Clause` | 否 | 无（同上） | sha256 `7fae4983…`；CI 内联 `sha256sum -c` | CI 不重生成（依赖 librime 版本）；脚本 `tools/generators/gen_key_golden.sh` |

- 金样清单、transcript 格式、每份金样的来源 pin 与 sha256 表见 \
  [`../goldens/README.md`](../goldens/README.md)。
- 探针用例矩阵 `tools/cases/*.txt` 与探针 `tools/probes/*.cpp`：本仓自写、不随包、无生成器 \
  （探针依赖系统 librime / librime-lua，用法见 [`../goldens/README.md`](../goldens/README.md)）。

## 6. 源码生成物（不随包，编进插件）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `crates/hux-core/src/key_table.rs` | RIME Developers；由 librime `src/rime/key_table.cc` @「键名表上游」<br>经 `gen_key_table.py` 生成，勿手改 | 键名与修饰位表 | `BSD-3-Clause` | 否（编进 `libhux.so`） | 无 | 文件头部自述来源 sha256 | `tools/generators/gen_key_table.py`（命令见本节末） |

- **本节守卫**： \
  `python3 tools/generators/gen_key_table.py --source <librime>/src/rime/key_table.cc --out /tmp/key_table.rs && diff /tmp/key_table.rs crates/hux-core/src/key_table.rs`（ \
  CI `golden` 同）。

## 7. 文档图片（不随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `docs/images/虎句.png`<br>`docs/images/虍.png`<br>`docs/images/配置页.png`<br>`docs/images/音反查.png`<br>`docs/images/字反查.png` | 本仓截图（明雅流风） | README / 文档插图 | `GPL-3.0-or-later` | 否 | 无 | 无内容校验（登记 / 许可由 `check_resources.py` 核对） | 人工截图，无生成器 |

## 8. 插件元数据与配置（随包）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `platform/fcitx5/conf/hux.addon.conf`<br>`platform/fcitx5/conf/hux.inputmethod.conf` | 本仓（明雅流风） | fcitx5 addon 元数据与输入法条目<br>（`Category=InputMethod`、`Library=libhux`、`OnDemand`） | `GPL-3.0-or-later` | 是 | 系统级 `<prefix>/share/fcitx5/{addon,inputmethod}/hux.conf`<br>用户级（`-u`）同构于 `~/.local/share/fcitx5/…`<br>（都改名 `hux.conf`） | CI `addon` 按 `DESTDIR` 查安装的两个 conf + `check_uninstall_clean.py` | 随仓手写，随元数据与落点演进；addon 契约见 [`../platform/README.md`](../platform/README.md) |

## 9. 许可证文本

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `LICENSE` | GNU/FSF 的 GPL-3.0 正文；本仓随项目发布 | 项目许可正文（GPL-3.0，供 GitHub 识别） | `GPL-3.0-or-later` | 否 | 无 | `reuse lint`（REUSE 忽略根许可正文） | 随项目许可变更替换正文 |
| `LICENSES/GPL-3.0-or-later.txt`<br>`LICENSES/GPL-3.0-only.txt`<br>`LICENSES/CC-BY-4.0.txt`<br>`LICENSES/BSD-3-Clause.txt`<br>`LICENSES/LicenseRef-HuMa-Official.txt` | SPDX 官方正文镜像（各发布方，<https://spdx.org/licenses/>）；`LicenseRef-HuMa-Official.txt` 为本仓自写说明 | 各许可全文（项目代码 / 派生数据 / 词先验 / 键名表 / 追加码表） | `GPL-3.0-or-later`<br>`GPL-3.0-only`<br>`CC-BY-4.0`<br>`BSD-3-Clause`<br>`LicenseRef-HuMa-Official` | 否 | 无 | `reuse lint` + 与 SPDX 头 / [`../REUSE.toml`](../REUSE.toml) 对应 | SPDX 列表重取正文；`LicenseRef-HuMa-Official.txt` 本仓撰写 |
| `REUSE.toml` | 本仓（明雅流风） | 无 SPDX 头的二进制 / 第三方文件的许可与版权标注 | `GPL-3.0-or-later` | 否 | 无 | `reuse lint`；许可一致性由 `check_resources.py` 比对 | 新增随包资源时同步补标注 |

## 10. 运行时可写数据（不随包，卸载对账用）

| 资源（默认路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `tiger_sentence.options.yaml` | 引擎运行时写入（选项持久化） | 选项落盘（合并顺序：本文件 > 设置 > 内建缺省） | `GPL-3.0-or-later`（运行时产物） | 否 | 用户级 `~/.local/share/fcitx5/hux/tiger_sentence.options.yaml` | 无内容校验 | 运行时写入，无生成器 |
| `tiger_sentence_learning_<hash>.userdb/` | 引擎运行时写入（LevelDB 学习库） | 选词学习与打分（键 `e/%010d`；`<hash>` = 方案 id 哈希） | `GPL-3.0-or-later`（运行时产物） | 否 | 用户级 `~/.local/share/fcitx5/hux/tiger_sentence_learning_<hash>.userdb/` | 无内容校验（缺省保留） | 运行时写入，无生成器 |
| `conf/hux.conf` | fcitx5 配置页与状态菜单写入 | 配置项落盘（配置页「虎虚」页；与选项存储双向同步） | 无（用户配置） | 否 | 用户级 `~/.config/fcitx5/conf/hux.conf` | 无内容校验 | fcitx5 配置页与状态菜单写入 |

- 三者都不随包、不进 `data/MANIFEST`；登记只为「卸载干净」对账（见 [`usage.md`](usage.md)）。
- 安装脚本不写持久文件，除 `-u` 的 `~/.config/environment.d/90-hux.conf`。

## 10b. 安装模式写入的配置（仅 `install.sh -u`）

| 资源（仓库路径） | 来源 | 作用 | 许可 | 随包 | 默认去向 | 校验 | 再生 / 更新 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `~/.config/environment.d/90-hux.conf` | 本仓安装脚本生成（非仓库文件） | 让 systemd 用户实例为 fcitx5 带上 `FCITX_ADDON_DIRS`<br>（值与原因见 [`usage.md`](usage.md)） | 无（用户环境配置） | 否 | 用户级 `~/.config/environment.d/90-hux.conf` | 写入幂等（相同不改写）；`uninstall.sh` 删除；生效需重登或启动前 export | `install.sh -u` 生成（幂等） |

- 写入内容、生效条件与未继承时的处理见 [`usage.md`](usage.md)「用户级（`-u`）的环境变量」。

两种安装模式（`-s` `/usr`、`-u` `$HOME/.local`）的落点与前提见 [`usage.md`](usage.md)； \
用户级下第 1 / 3 / 4 / 8 节的 `<prefix>` 即 `$HOME/.local`。

## 11. 后台服务与端口

**本引擎无常驻服务、无 socket、无端口**：

- addon 是 fcitx5 进程内的动态库（`libhux.so`，`OnDemand=True`）：无守护进程、不监听端口、不与外部通信。
- 学习库是同进程内的 LevelDB（`…userdb/`），只用文件锁。
- 日志走 fcitx5 设施（类别 `hux`）；安装脚本不自动重启 fcitx5，不产生脚本侧日志。
- 进程环境覆盖只有 `HUX_DATA_DIRS`（数据）与 `HUX_MODEL`（模型）；其余 `HUX_*` 只见于测试与探针。

## 12. 未随包 / 未使用的第三方

| 项目 | 用途 | 现状 |
| --- | --- | --- |
| [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime) 的 Lua 核心 | 测试 oracle：生成金样、对照语义 | 不进运行时；检出不入库（`_external/`，CI 临时检出） |
| librime / librime-lua | 金样探针（`tools/probes/*.cpp`）与键名表来源 | 不链接、不依赖；仅测试期与生成期用 |
| [rime-mohu](https://github.com/fcxxxz/rime-mohu) | 词先验位图的上游词库 | 只经派生位图间接使用（`CC-BY-4.0`，署名见上节） |
| [hufu-ime-rust](https://github.com/LeafHW/hufu-ime-rust) | 共享主题的来源 | 只用转换后的主题产物 |
| 真实 n-gram 模型与本地抽样金样（`goldens/local/`） | 性能基准与真实模型差分 | 均不入库（模型自取，抽样金样本地生成） |

**校验**：依赖边界（内核不依赖方案、平台不引用方案内部模块）由 CI 层依赖守卫与 `cargo tree` \
判定，见 [`design.md`](design.md) §4（依赖校验）。

## 自动校验

| 守卫命令 | 覆盖对象 | CI 作业 |
| --- | --- | --- |
| `python3 tools/checks/check_resources.py` | 本页登记 / 许可 ↔ 仓库资源 | `rust` |
| `bash tools/checks/check_data_manifest.sh` | 清单 ↔ `data/` ↔ 装 / 卸 / CMake | `rust` |
| `python3 tools/checks/check_code_tables.py` | 码表命名 / 行格式 / 去重 / 只补缺字 | `rust` |
| `python3 tools/checks/verify_golden_shas.py` | 金样表 ↔ 文件 ↔ 头部 | `rust` |
| `python3 tools/checks/verify_golden_shas.py --reference _external/tiger-sentense-rime` | 参照检出 `lua/*`、`tools/*` | `golden` |
| `python3 tools/checks/check_branding_assets.py` | 主源 / SVG / 位图 / 聚合指纹 | `rust` |
| `python3 tools/checks/check_themes.py` | 主题清单 ↔ 目录 ↔ 文件 ↔ 引用图 | `rust` |
| `python3 tools/checks/check_uninstall_clean.py` | 安装集合 ⊆ 可卸载集合 | `addon` |
| `python3 tools/generators/gen_key_table.py` + `diff` | `key_table.rs` ↔ librime pin 重生成 | `golden` |
| 夹具类金样重生成 + 逐字节比对 | 夹具 / decode / learning / lexical 金样 | `golden`、`golden-lua-latest` |
| CI 内联 `sha256sum -c` | 词先验、音反查索引与四份不重生成金样 | `rust` |
| `reuse lint` | 逐文件许可 / 版权标注 | `reuse` |

新增随包资源（数据 / 主题 / 图形 / 插件 conf）： \
先落清单（`data/MANIFEST` 或 `assets/themes/MANIFEST`）再在本页补一行； \
文档图片、许可证文本同样补一行。只落清单会被 `check_resources.py` 拦下（宽 glob 不算； \
`goldens/**` 按组）。
