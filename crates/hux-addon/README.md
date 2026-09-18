<!-- SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# hux-addon（K3）

fcitx5 addon：**C++ 薄壳**（`shell/`，只做 fcitx5 接口适配）+ **Rust 逻辑**（`src/`，经 C ABI 调用
`hux-core`）。按键 → core（`processor`/`translate`）→ 提交 / preedit / 候选 → fcitx5。

按键语义与参照（librime）一致：组合中的可打印字符（如大写字母）先提交当前组合，再交应用；
为保证上屏顺序，宿主层会消费该键并以 `forwardKey` 重发——客户端先收到提交、后收到按键。
**例外**：布局转换键（系统布局与方案布局不同时，如系统 colemak + 方案 us）交回核心处理，
由核心提交**转换后**的字符；自行转发会让客户端按系统布局重新解释该键。

## 构建与安装

```sh
cmake -S crates/hux-addon -B build/addon -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/addon -j
sudo cmake --install build/addon      # /usr/lib/fcitx5/libhux.so + 两个 conf
```

安装后重启 fcitx5（`fcitx5 -r -d`），在配置工具中添加「hux」（方案：虎句）。

## 数据目录

随包数据在仓库 `data/`（码表四件套、`tiger_sentence.lexical.bin`、`tiger_sentence.pinyin.bin.gz`、
`symbols.yaml`）；运行时按 `~/.local/share/fcitx5/hux` → `/usr/share/fcitx5/hux` 查找，可选模型另在
`models/sentence-ngram-mobile.bin` 查找。安装到用户目录：

```sh
mkdir -p ~/.local/share/fcitx5/hux/models
cp data/tiger_sentence.* data/symbols.yaml ~/.local/share/fcitx5/hux/
```

开发可用环境变量覆盖（目录冒号分隔 / 模型路径；`data/` 已含全部随包数据）：

```sh
HUX_DATA_DIRS="data" \
HUX_MODEL="$HOME/.local/share/fcitx5/rime/models/sentence-ngram-mobile.bin" \
fcitx5 -r -d
```

（`HUX_MODEL` 示例取自 fcitx5-rime 数据目录，按实际安装位置替换；任意 TCSKNM02 模型均可。）

模型（可选）可与 fcitx5-rime **共用同一份**（来源：[Releases › model](https://github.com/lvyww/tiger-sentense-rime/releases/tag/model)）：
实体放 hux 数据目录，再在 rime 共享目录建软链（rime 的查找顺序为 用户 `models/` → 用户根 → 共享 `models/`）：

```sh
sudo mkdir -p /usr/share/rime-data/models
sudo ln -s /usr/share/fcitx5/hux/models/sentence-ngram-mobile.bin \
           /usr/share/rime-data/models/sentence-ngram-mobile.bin
```

## 配置（fcitx5-configtool 设置页）

设置页分「行为」「快捷键」两个分区（与 fcitx5 全局设置同款；选项详情可悬浮查看）。

**行为**（布尔项在前，值选项在后）

| 项 | 默认 | 说明 |
|---|---|---|
| EarlyCommit / EarlyCommitToPreedit / AllowDuplicateSingle | 开/关/开 | 早提交三项 |
| FullShape / AsciiPunct | 关/关 | 全角标点 / ASCII 标点直通 |
| TabLearning | 开 | Tab 选字写学习库 |
| DigitSelect | 关 | 数字直选：`1`–`9` 上屏当前页候选，`0`=第 10 个 |
| PanelPreedit | 关 | 候选窗口显示预编辑文本（仅宿主显示项，不经引擎）|
| HighFreqLimit | 1500 | 高频字过滤上限（重启生效）|
| PageSize | 5 | 每页候选个数（1–10）|

**快捷键**（`KeyList`，均可多项）

| 项 | 默认 | 说明 |
|---|---|---|
| SoundToCharShapeKey | Alt+`:` | 音反查（拼音 → 虎码）|
| CharToSoundShapeKey | Alt+`"` | 字反查（光标左侧汉字的拼音与虎码）|
| PageUpKey / PageDownKey | `-` / `=` | 翻页（上：翻页中生效；下：有候选时生效）|

快捷键为 fcitx5 `KeyList`（配置工具与「全局设置」同款，可配置多项；`AllowModifierLess`
允许 `` ` ``、`;` 等无修饰键）。
未显式提供的项跟随 fcitx5 全局设置（候选列表方向、客户端内联预编辑）。

## 反查

音反查与字反查同机制：触发键推入组合；**仅当触发键为单字符键**（无 Ctrl/Alt/Super）时给出
默认可上屏候选（触发字符按标点表取半/全角，空格上屏），带修饰键的触发不给默认候选。

- **音反查**：输入拼音（支持拼写缩写）出虎码候选；预编辑按音节切分（`` `zhongguo `` → `` `zhong guo ``）。
- **字反查**：取应用侧周边文本（应用不可用时查不到内容、两排为空，不做提示）；两排显示光标
  左侧 1 个字——上排（排头「咅」）= 拼音、下排（排头「虍」）= 虎码（多音/多码以 `/` 连接，缺数据 `?`）；
  ←/→/↑/↓ 交应用处理（应用光标随动，本层不消费；查码段不下发预编辑，避免应用端 marked text 锁住光标）；
  Esc / 再次触发 / 其它键退出（打字照常输入）。展示面为输入面板辅助文本条（auxUp/auxDown）。

## 选项与学习

- 选项：`~/.local/share/fcitx5/hux/tiger_sentence.options.yaml` 为主存储（YAML，未知键保留），缺失键
  回退 `user.yaml` 的 `var/option/<name>`（只读）；保存失败写属性 `tiger_sentence_options_error`。
- 学习库：`~/.local/share/fcitx5/hux/tiger_sentence_learning_<hash(schema_id)>.userdb/`
  （LevelDB 同构、可直接迁移；1 万条 / 16 MiB，60 秒节流刷新）；提交点通知器由 core 负责，
  宿主排空 `LiveLearning::submitted` 落库。

## 已知限制

每引擎单会话（切换/重置即清空）；候选为展示型（点击不提交）；打包与状态菜单待做。
