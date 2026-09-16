# tigerclaw-addon（K3）

fcitx5 addon：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 逻辑**（`src/`，经 C ABI
调用 `tigerclaw-core`）。按键 → Rust（core `processor`/`translate`）→ 提交 / preedit / 候选 → fcitx5。

## 构建与安装

```sh
cmake -S crates/tigerclaw-addon -B build/addon \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon      # /usr/lib/fcitx5/libtigerclaw.so + 两个 conf
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

## 选项

`~/.local/share/fcitx5/tigerclaw/tiger_sentence.options.yaml` 为主存储（YAML，未知键保留）；
缺失键回退 `user.yaml` 的 `var/option/<name>`（只读）。保存失败写入属性
`tiger_sentence_options_error`。

## 学习

`~/.local/share/fcitx5/tigerclaw/tiger_sentence_learning_<hash(schema_id)>.userdb/`
（LevelDB：键 `e/%010d`、值 = frame 五元组；上限 1 万条 / 16 MiB；`refresh_scores` 60 秒节流，
与 Rime 同构、可直接迁移）。提交点的通知器序列由 core 负责，宿主排空
`LiveLearning::submitted` 落库。

## 状态

- K3a：注册（addon/输入法条目 conf）+ 按键回路；
- K3b：core 会话接线——fcitx5 状态 → core（Rime）掩码、`CompositionBuilder` 组合重建
  （提交或输入变化时重建）、提交 / preedit（字节光标）/ 候选与高亮、
  `activate/deactivate/reset` 生命周期、`_auto_commit`；
- K3d：选项持久化（`options.yaml` + legacy 回退 + 错误属性）；
- K3e：学习库（LevelDB 落库 + 节流刷新 + `_hide_candidate` / `ascii_mode` 确认）；
- K3f（⑥）：宿主编辑语义——core `host` 模块（librime `key_binder`/`selector`/`navigator`/
  `express_editor` 等价物）+ 组合重建随光标（`CompositionBuilder` 参照 `ConcreteEngine::Compose`）；
  2c 键序列金样扩到 36 例/200 步（编辑/导航键、缓冲/锁定态）；
- K3g（⑦a）：ascii_composer——core `ascii` 模块（Shift 轻击/Caps 切换 `ascii_mode`、
  `commit_code`/`commit_text`/`clear` 样式、ascii 直通、`good_old_caps_lock`）；
  金样扩到 42 例/237 步（CapsLock 为 fcitx5 适配：不按键切换，不入金样）。

已知限制（后续增量）：每引擎单会话（切换/重置即清空）；候选为展示型（点击不提交）；
标点表（⑦b）、反查（⑧）、状态菜单与打包（⑨）待做。
