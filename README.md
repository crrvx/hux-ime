# hux-ime

虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。
计算与交互核心全部为 Rust，不依赖 librime。

名称 `hux` = `tux`(linux) + `hu`(虎码)

## 文档

| 文档                                                        | 内容                                       |
| ----------------------------------------------------------- | ------------------------------------------ |
| [`docs/rust-migration.md`](docs/rust-migration.md)         | 设计：路线与状态、模块映射、数据、集成要点 |
| [`crates/hux-addon/README.md`](crates/hux-addon/README.md) | addon 构建/安装、数据目录、配置与功能说明  |
| [`goldens/README.md`](goldens/README.md)                   | 差分金样：清单、来源、复现命令             |
| [`data/README.md`](data/README.md)                         | 随包数据说明                               |

## 开发

依赖：Rust 1.85+（edition 2024）。

```sh
cargo test -p hux-core -p hux-addon       # 逐位差分 + addon 测试（本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

## 安装（fcitx5 addon）

依赖：CMake 3.20+、fcitx5 开发包（`Fcitx5Core`）。

```sh
cmake -S crates/hux-addon -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon
fcitx5 -r -d  # 或以所在发行版的方式重启
```

随包数据在仓库 [`data/`](data/README.md)（码表四件套、词先验位图、音查虎索引、标点表），
安装到 fcitx5 数据目录：

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp data/tiger_sentence.* data/symbols.yaml ~/.local/share/fcitx5/hux/
```

可选 n-gram 模型不随包（来源 [Releases › model](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model)，
可与 fcitx5-rime 共用同一份）；随后在配置工具中添加「hux」，
细节见[`crates/hux-addon/README.md`](crates/hux-addon/README.md)。

## 使用

方案：虎句（`tiger_sentence`）。

`空格`上屏高亮、`Tab`/`Shift+Tab` 移动高亮，
`=`/`-` 或 `PgDn`/`PgUp` 翻页，
`回车`提交原文，`Esc` 取消。

> 每页 5 个候选项

两个反查助手如下，可在配置工具的「hux」页修改（`PinyinLookupKey`/`CharacterLookupKey`）。

### 音查虎：拼音 → 虎码（默认 `Alt`+`:`）

按下触发键后输入拼音（支持拼写缩写），候选即为对应词语，注释显示虎码；预编辑按音节切分，
空格上屏高亮候选：

```
Alt+:  zhongguo   →   :zhong guo〔拼音〕   候选：中国 …
```

### 字查音+虎：查光标左侧汉字的音与码（默认 `Alt`+`"`）

按下触发键后，输入面板显示光标左侧 1 个字的信息：
上排拼音（排头「**咅**」）、下排虎码（排头「**虍**」）

> 多音/多码以 `/` 连接，缺数据为 `?`

```
咅 zhong
虍 d/dg/dgs
```

依赖应用的周边文本支持（不支持时上排提示「应用不支持周边文本」）；`←`/`→` 移动应用光标
（提示随光标刷新），`Esc`、再次触发或输入其它键退出。

## 致谢

感谢以 [B佬（lvyww）](https://github.com/lvyww) 为首的一众维护者对虎码的支持。

- 虎码官网：[tiger-code.com](https://www.tiger-code.com)

虎码信息汇总：

- 官方 · [虎娘](https://github.com/lvyww/tigirl)
- 官方 · [虎爪](https://github.com/lvyww/tigerclaw)
- 官方 · [虎爪-rime](https://github.com/lvyww/tiger-sentense-rime)（本方案的参照实现，GPL-3.0）
- 社区 · [虎符](https://github.com/LeafHW/hufu-ime-rust)
- 社区 · [魔虎音形](https://github.com/fcxxxz/rime-mohu)
- 社区 · hux（本方案）

## 许可与署名

- 代码：GPL-3.0-or-later，见 [`LICENSE`](LICENSE)。
- 码表数据 `data/tiger_sentence.{codes,char_ranks,full_code_whitelist,supplement}.txt`：取自上游
  [tiger-sentense-rime](https://github.com/lvyww/tiger-sentense-rime)（GPL-3.0）。
- 词先验数据 `data/tiger_sentence.lexical.bin`：**CC BY 4.0**（派生自
  [rime-mohu](https://github.com/fcxxxz/rime-mohu)）；署名见
  [`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)，许可正文见
  [`LICENSES/CC-BY-4.0.txt`](LICENSES/CC-BY-4.0.txt)。
