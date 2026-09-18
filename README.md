<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# hux-ime

**虎句**（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。 \
计算与交互核心全部为 Rust，不依赖 librime。

名称 **hux** = **tux**`linux` + **hu**`虎码`

## 文档

| 文档                                                       | 内容                                             |
| ---------------------------------------------------------- | ------------------------------------------------ |
| [`docs/usage.md`](docs/usage.md)                           | 开发 / 安装 / 使用 / 卸载                        |
| [`docs/config.md`](docs/config.md)                         | 配置项：行为 / 快捷键 / 选项与学习存储           |
| [`docs/rust-migration.md`](docs/rust-migration.md)         | 设计：路线与状态、模块映射、数据、集成要点       |
| [`crates/hux-addon/README.md`](crates/hux-addon/README.md) | addon 实现：分工、按键语义、反查机制、已知限制   |
| [`goldens/README.md`](goldens/README.md)                   | 差分金样：清单、来源、复现命令                   |
| [`data/README.md`](data/README.md)                         | 随包数据说明                                     |

## 致谢

**虎码** 由 [B佬（lvyww）](https://github.com/lvyww) 创作。感谢以**B佬**为首的一众虎码同志对虎码的维护与贡献。

本项目的语义、数据与金样均以[虎爪-rime](https://github.com/lvyww/tiger-sentense-rime) 为参照，谨致谢意。

hux-ime 是兴趣驱动的 fcitx5 社区方案，与上游无隶属关系。

## 虎码信息汇总

- 虎码官网：[tiger-code.com](https://www.tiger-code.com)
- 虎码资源：[huma.ysepan.com](https://huma.ysepan.com)

<br>

- 官方 · [虎娘](https://github.com/lvyww/tigirl)
- 官方 · [虎爪](https://github.com/lvyww/tigerclaw)
- 官方 · [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)（本方案的参照实现，GPL-3.0）

<br>

- 社区 · [虎符](https://github.com/LeafHW/hufu-ime-rust)
- 社区 · [魔虎音形](https://github.com/fcxxxz/rime-mohu)
- 社区 · [hux](https://github.com/crrvx/hux-ime)（本方案）

## 许可与署名

- 代码：GPL-3.0-or-later，见 [`LICENSE`](LICENSE)。
- 拼音数据 `data/tiger_sentence.pinyin.bin.gz` 取自 [虎码官方秃版小狼毫](https://huma.ysepan.com)。
- 词先验数据 `data/tiger_sentence.lexical.bin`：[CC-BY-4.0](LICENSES/CC-BY-4.0.txt) 派生自 [rime-mohu](https://github.com/fcxxxz/rime-mohu)；
- 模型/码表/其他数据 `data/tiger_sentence.*`：[GPL-3.0](LICENSES/GPL-3.0-only.txt) 取自 [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)；
  词先验署名见 [`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)。
- 各文件版权与许可以 SPDX 头 / [`REUSE.toml`](REUSE.toml) 标注（REUSE 规范），许可正文见 [`LICENSES/`](LICENSES/)。
