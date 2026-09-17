# hux-ime

虎句（`tiger_sentence`）输入方案的 fcitx5 原生 Rust 实现。计算与交互核心全部为 Rust，不依赖
librime（参照实现仅用于差分测试）。当前处于 K3（fcitx5 addon）收尾阶段，K4 验收待做。

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/rust-migration.md`](docs/rust-migration.md) | 设计：路线与状态、模块映射、数据、集成要点 |
| [`crates/hux-addon/README.md`](crates/hux-addon/README.md) | addon 构建/安装、数据目录、配置与功能说明 |
| [`goldens/README.md`](goldens/README.md) | 差分金样：清单、来源、复现命令 |
| [`data/README.md`](data/README.md) | 随包数据说明 |

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

安装后需在 fcitx5 数据目录准备随包数据（码表、词先验位图、音查虎索引，可选 n-gram 模型），并
在配置工具中添加「hux」；详见 [`crates/hux-addon/README.md`](crates/hux-addon/README.md)。

## 许可与署名

- 代码：GPL-3.0-or-later，见 [`LICENSE`](LICENSE)。
- 词先验数据 `data/tiger_sentence.lexical.bin`：**CC BY 4.0**（派生自
  [rime-mohu](https://github.com/fcxxxz/rime-mohu)）；署名见
  [`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)，许可正文见
  [`LICENSES/CC-BY-4.0.txt`](LICENSES/CC-BY-4.0.txt)。
- 参照实现（测试 oracle）：[tiger-sentense-rime](https://github.com/crrvx/tiger-sentense-rime)（GPL-3.0）。
