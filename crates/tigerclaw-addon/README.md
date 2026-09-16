# tigerclaw-addon（K3）

fcitx5 addon：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 逻辑**（`src/`，经 C ABI
调用 `tigerclaw-core`）。按键 → Rust（core `processor`/`translate`）→ 提交 / preedit / 候选 → fcitx5。

## 构建与安装

```sh
cmake -S crates/tigerclaw-addon -B build-addon \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build-addon -j
sudo cmake --install build-addon      # /usr/lib/fcitx5/libtigerclaw.so + 两个 conf
```

安装后重启 fcitx5（`fcitx5 -r -d`），在配置工具中添加「虎整句」。

## 数据目录

默认按 `~/.local/share/fcitx5/tigerclaw` → `/usr/share/fcitx5/tigerclaw` 查找
（码表四件套、`tiger_sentence.lexical.bin`、可选 `models/sentence-ngram-mobile.bin`）。
开发可用环境变量覆盖（目录冒号分隔 / 模型路径）：

```sh
TIGERCLAW_DATA_DIRS="$PWD/goldens/lexicon:$PWD/data" \
TIGERCLAW_MODEL="$HOME/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

开发数据也可直接铺到用户目录（免环境变量）：

```sh
mkdir -p ~/.local/share/fcitx5/tigerclaw/models
cp goldens/lexicon/*.txt data/tiger_sentence.lexical.bin ~/.local/share/fcitx5/tigerclaw/
ln -sf ~/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin \
  ~/.local/share/fcitx5/tigerclaw/models/
```

## 状态

- K3a：注册（addon/输入法条目 conf）+ 按键回路；
- K3b：core 会话接线——fcitx5 状态 → core（Rime）掩码、`processor` + `translate` 组合重建
  （照 2c 重放桩规则：提交或输入变化时重建）、提交 / preedit（字节光标）/ 候选与高亮、
  `activate/deactivate/reset` 生命周期、`_auto_commit` 与核心选项缺省。

已知限制（后续增量）：每引擎单会话（切换/重置即清空）；候选为展示型（点击不提交）；
数据/选项/学习/反查与打包见 K3 其余项。
