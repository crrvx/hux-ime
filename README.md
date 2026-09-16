# tigerclaw-fcitx5

虎整句输入方案：fcitx5 原生 Rust 实现（Rust 直迁进行中，K0 已完成）。

| 文档 | 内容 |
|---|---|
| [`docs/rust-migration.md`](docs/rust-migration.md) | 设计：路线 K0–K4、仓库结构、模块映射、集成要点 |
| [`docs/rime-semantics.md`](docs/rime-semantics.md) | 参照实现行为对照（移植用） |
| [`docs/spike-report.md`](docs/spike-report.md) | K0 结果：逐位验证、性能、陷阱审计 |
| [`goldens/README.md`](goldens/README.md) | 差分金样与复跑命令 |

## 开发

```sh
cargo test -p tigerclaw-core                        # 逐位差分（fixture；本地抽样缺失自动跳过）
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

参照实现（测试 oracle）：[tiger-sentense-rime](https://github.com/crrvx/tiger-sentense-rime)（GPL-3.0）。

## 许可

GPL-3.0-or-later，见 [`LICENSE`](LICENSE)。

随包的词先验数据 `data/tiger_sentence.lexical.bin` 为 **CC BY 4.0**（派生自
[rime-mohu](https://github.com/fcxxxz/rime-mohu)）：署名与来源见
[`docs/LEXICAL_PRIOR_ATTRIBUTION.md`](docs/LEXICAL_PRIOR_ATTRIBUTION.md)，
许可正文见 [`LICENSES/CC-BY-4.0.txt`](LICENSES/CC-BY-4.0.txt)。
