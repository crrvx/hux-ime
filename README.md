# hux-ime

虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现（K3 addon 进行中，K4 验收待做）。

| 文档 | 内容 |
|---|---|
| [`docs/rust-migration.md`](docs/rust-migration.md) | 设计：路线与状态、模块映射、数据、集成要点 |
| [`crates/hux-addon/README.md`](crates/hux-addon/README.md) | addon 构建/安装/配置与功能说明 |
| [`goldens/README.md`](goldens/README.md) | 差分金样：清单、来源、复现命令 |
| [`data/README.md`](data/README.md) | 随包数据说明 |

## 构建与测试

```sh
cargo test -p hux-core -p hux-addon       # 逐位差分 + addon 测试（本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

## 安装（fcitx5 addon）

```sh
cmake -S crates/hux-addon -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j && sudo cmake --install build/addon
fcitx5 -r -d
```

## 许可与署名

- 代码：GPL-3.0-or-later，见 [`LICENSE`](LICENSE)。
- 词先验数据 `data/tiger_sentence.lexical.bin`：**CC BY 4.0**（派生自
  [rime-mohu](https://github.com/fcxxxz/rime-mohu)）；署名见
  [`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)，许可正文见
  [`LICENSES/CC-BY-4.0.txt`](LICENSES/CC-BY-4.0.txt)。
- 参照实现（测试 oracle）：[tiger-sentense-rime](https://github.com/crrvx/tiger-sentense-rime)（GPL-3.0）。
