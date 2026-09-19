<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 虎虚输入法 / hux-ime

**虎句**（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。 \
计算与交互核心全部为 Rust，不依赖 librime。

> 本项目的语义、数据与金样均参照 [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)。\
> hux-ime 属于社区方案，出于热爱而作，与上游无隶属关系。

- 英文名 —— **hux** =  **hu**`虎码` + **tux**`linux(意指fcitx5方案)`
- 中文名 —— **虎虚** = **虎**`hux正取2字母` + **虚**`hux倒取2字母`

## 文档

| 文档                                                       | 内容                                             |
| ---------------------------------------------------------- | ------------------------------------------------ |
| [`docs/usage.md`](docs/usage.md)                           | 开发 / 安装 / 使用 / 卸载                        |
| [`docs/config.md`](docs/config.md)                         | 配置项：行为 / 快捷键 / 选项与学习存储           |
| [`docs/rust-migration.md`](docs/rust-migration.md)         | 设计：路线与状态、模块映射、数据、集成要点       |
| [`docs/config-options.md`](docs/config-options.md)         | 计划：新增可配置项（A 组四项）                   |
| [`docs/android.md`](docs/android.md)                       | 计划：fcitx5-android 插件适配                    |
| [`crates/hux-addon/README.md`](crates/hux-addon/README.md) | addon 实现：分工、按键语义、反查机制、已知限制   |
| [`goldens/README.md`](goldens/README.md)                   | 差分金样：清单、来源、复现命令                   |
| [`data/README.md`](data/README.md)                         | 随包数据说明                                     |

## 虎码信息汇总

- 虎码官网：[tiger-code.com](https://www.tiger-code.com)
- 虎码资源：[huma.ysepan.com](https://huma.ysepan.com)
- 虎句方案：
  - 官方 · [虎娘](https://github.com/lvyww/tigirl)
  - 官方 · [虎爪](https://github.com/lvyww/tigerclaw)
  - 官方 · [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)（本方案的参照实现）
  - 社区 · [虎符](https://github.com/LeafHW/hufu-ime-rust)
  - 社区 · [虎虚 hux](https://github.com/crrvx/hux-ime)（本方案）

## 致谢

特别感谢 [B佬（lvyww）](https://github.com/lvyww) 对 **虎码** 的发明！也感谢倾情虎码建设的各位同志！\
若没有同志们的鼎力相助，就没有如今 **虎码** 在 **虎字、虎词、虎句** 等方案上的一路高歌，推陈出新！

## 署名

- 拼音数据 `data/tiger_sentence.pinyin.bin.gz` 转换自 [虎码官方秃版小狼毫](https://huma.ysepan.com)。
- 词先验数据 `data/tiger_sentence.lexical.bin`：[CC-BY-4.0](LICENSES/CC-BY-4.0.txt)
  派生自 [rime-mohu](https://github.com/fcxxxz/rime-mohu)；\
  署名见 [`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)。
- 模型/码表/其他数据 `data/tiger_sentence.*`：[GPL-3.0](LICENSES/GPL-3.0-only.txt)
  取自 [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)。

## 许可证

本项目代码以 「**GPL-3.0-or-later**」 发布，全文见 [`LICENSE`](LICENSE)。\
各文件的版权与许可经 SPDX 头 / [`REUSE.toml`](REUSE.toml) 标注（REUSE 规范），
许可正文见 [`LICENSES/`](LICENSES/)。
