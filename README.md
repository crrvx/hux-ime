<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 虎虚输入引擎 / hux-ime

**虎句**（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现； \
计算与交互核心全部为 Rust，不依赖 librime。

> 语义、数据与金样均参照 [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)； \
> hux-ime 是社区方案，出于热爱而作，与上游无隶属关系。

just for fun:

| 名     | 解释                                                                     |
| ------ | ------------------------------------------------------------------------ |
| 英文名 | **hux** = **hu**`虎码` + **tux**`linux(意指 fcitx5 方案)`                |
| 中文名 | **虎虚** = **虎**`hux 正取 2 字母` + **虚**`hux 倒取 2 字母`             |
| 托盘图 | 虎虚虎虚，**虎内空虚**，故取 **虍**；字反查 **虍** 内取空、**咅** 也取空 |

![虍](./docs/images/虍.png)

## 快速指南

```sh
git clone https://github.com/crrvx/hux-ime && cd hux-ime

# 预演：./install.sh --dry-run
./install.sh -s  # 系统级安装（缺省）：构建 → 装插件 / 数据 / 图标 / 主题
./install.sh -u  # 用户级安装：全部装到 ~/.local（并写 environment.d 让 fcitx5 找到插件）
```

装完按提示重启 fcitx5：`nohup fcitx5 -r -d >/dev/null 2>&1 &`； \
随后在 fcitx5 配置工具「添加输入法」→ **虎虚（hux）**。

**模型（可选，自取）**：n-gram 模型不随包，能明显提升整句质量； \
从 [上游](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model) 取 `sentence-ngram-mobile.bin`，放于系统级目录或用户级目录 \
不装也能用，仅整句排序略弱；文件名与校验值见 [`docs/resources.md`](docs/resources.md) §2。

- 用户级目录：`~/.local/share/fcitx5/hux/models/`（推荐）
- 系统级目录：`/usr/share/fcitx5/hux/models/`

**基本键位**

- `空格` 上屏高亮项；
- `←/→` 移动光标；
- `↑/↓` 或 `Tab / Shift+Tab` 选字；
- `1`–`9` 直选当页候选（`0` = 第 10 个）；
- `PgUp/PgDn` 翻页
- `Enter` 提交原文，`Esc` 取消
- `` ` `` 音反查（拼音 → 虎码）；
- `~` 字反查（光标左侧汉字 → 拼音 + 虎码）；
- 托盘右键菜单，详见 [`docs/usage.md`](docs/usage.md)

> 本仓有意偏离上游缺陷，见 [`docs/upstream-deviations.md`](docs/upstream-deviations.md) ①

![虎句](./docs/images/虎句.png)

**一键卸载**

```sh
./uninstall.sh           # 交互式三问：主题 / 模型 / 用户数据（缺省只卸主题）
./uninstall.sh --dry-run # 预演：不提问、不删除，并打印计划删除清单
```

- 更多细节 [`docs/usage.md`](docs/usage.md)
- 配置项 [`docs/config.md`](docs/config.md)

## 文档

| 文档                                                         | 内容                                                                |
| ------------------------------------------------------------ | ------------------------------------------------------------------- |
| [`docs/usage.md`](docs/usage.md)                             | 指南：开发 / 安装 / 使用 / 卸载                                     |
| [`docs/config.md`](docs/config.md)                           | 配置项：行为 / 字集 / 快捷键 / 选项与学习存储                       |
| [`docs/design.md`](docs/design.md)                           | 设计 + 活规则：结构与硬规则 / 方案契约 / 依赖校验 / 骨架 / 模块映射 / 数据与目录 / fcitx5 集成 / 测试 / 性能             |
| [`docs/resources.md`](docs/resources.md)                     | 资源细则：来源（pin / sha）/ 许可 / 作用 / 随包与去向 / 再生与校验  |
| [`docs/review-ledger.md`](docs/review-ledger.md)             | 台账：未闭合与待定项（活口）+ 历史纪要      |
| [`docs/upstream-deviations.md`](docs/upstream-deviations.md) | 政策：有意偏离上游（①②③④）的依据、可证伪期望值表与回归做法      |
| [`platform/README.md`](platform/README.md)                   | 平台层：状态总览、Linux 构建安装、fcitx5 addon 契约、Android 计划   |
| [`crates/hux-scheme/README.md`](crates/hux-scheme/README.md) | 方案区：虎句现状与骨架方案的数据 / 契约需求                         |
| [`goldens/README.md`](goldens/README.md)                     | 金样：清单、transcript 格式、重新生成、来源与校验和（sha 表）、规则 |
| [`data/README.md`](data/README.md)                           | 随包数据：清单 / 格式 / 追加码表 / 来源与代价                       |
| [`assets/branding/README.md`](assets/branding/README.md)     | 品牌图形：主源、派生与校验                                          |
| [`assets/themes/README.md`](assets/themes/README.md)         | 共享主题：取用与更新步骤                                            |
| [`AGENTS.md`](AGENTS.md)                                     | 协作约定：流程 / 命名 / 代码与移植纪律 / 文档边界                   |

> **文档分工**（纪律见 [`AGENTS.md`](AGENTS.md)）：活文档只写**现状与做法**； \
> **历史与逐批记录**（含未闭合项、待定配置项）进 [`docs/review-ledger.md`](docs/review-ledger.md)； \
> **有意偏离上游**进 [`docs/upstream-deviations.md`](docs/upstream-deviations.md)。 \

## 虎码信息汇总

- 虎码官网：[tiger-code.com](https://www.tiger-code.com)
- 虎码资源：[huma.ysepan.com](https://huma.ysepan.com)
- 虎句方案：
  - 官方 · [虎娘](https://github.com/lvyww/tigirl) · [虎爪](https://github.com/lvyww/tigerclaw) · [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)（上游）
  - 社区 · [虎符](https://github.com/LeafHW/hufu-ime-rust) · [虎虚 hux](https://github.com/crrvx/hux-ime)（本方案）

## 致谢

特别感谢 [B佬（lvyww）](https://github.com/lvyww) 发明 **虎码**，也感谢倾情虎码建设的各位同志 —— \
没有同志们的鼎力相助，就没有 **虎字、虎词、虎句** 等方案的一路高歌。

## 署名

- 码表：转换自 [虎码官方秃版小狼毫](https://huma.ysepan.com)
- 模型：取自 [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)
- 词先验数据：派生自 [rime-mohu](https://github.com/fcxxxz/rime-mohu) \
  署名与复现：见 [`docs/resources.md`](docs/resources.md)

## 许可证

- 本项目代码以 「**GPL-3.0-or-later**」 发布，全文见 [`LICENSE`](LICENSE) \
  （许可正文无法区分 only/or-later，见各文件 SPDX 头与 `Cargo.toml`）
- 各文件的版权与许可经 SPDX 头 / [`REUSE.toml`](REUSE.toml) 标注（REUSE 规范） \
  许可正文见 [`LICENSES/`](LICENSES/)

如有任何改进建议，欢迎 [issue](https://github.com/crrvx/hux-ime/issues)
